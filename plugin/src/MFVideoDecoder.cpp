// SPDX-License-Identifier: AGPL-3.0-only
#include "MFVideoDecoder.h"

#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <mferror.h>
#include <propvarutil.h>
#include <wrl/client.h>

namespace dawvr
{
using Microsoft::WRL::ComPtr;

struct MFVideoDecoder::Impl
{
    juce::CriticalSection lock;
    juce::File currentFile;
    ComPtr<IMFSourceReader> sourceReader;
    int videoWidth{ 0 };
    int videoHeight{ 0 };
    double durationSeconds{ 0.0 };
    bool isBottomUp{ false };
    LONG stride{ 0 };

    juce::Image cachedImage;
    double cachedTime{ -1.0 };
    bool mfInitialized{ false };

    Impl()
    {
        HRESULT hr = MFStartup(MF_VERSION, MFSTARTUP_FULL);
        if (SUCCEEDED(hr))
            mfInitialized = true;
    }

    ~Impl()
    {
        close();
        if (mfInitialized)
        {
            MFShutdown();
            mfInitialized = false;
        }
    }

    void close()
    {
        const juce::ScopedLock sl(lock);
        sourceReader.Reset();
        currentFile = {};
        videoWidth = 0;
        videoHeight = 0;
        durationSeconds = 0.0;
        cachedImage = {};
        cachedTime = -1.0;
    }

    bool open(const juce::File& file)
    {
        const juce::ScopedLock sl(lock);
        close();

        if (!file.existsAsFile())
            return false;

        ComPtr<IMFAttributes> attributes;
        HRESULT hr = MFCreateAttributes(&attributes, 2);
        if (FAILED(hr))
            return false;

        // Enable video processing and low latency reading
        attributes->SetUINT32(MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, TRUE);
        attributes->SetUINT32(MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, TRUE);

        ComPtr<IMFSourceReader> reader;
        hr = MFCreateSourceReaderFromURL(file.getFullPathName().toWideCharPointer(), attributes.Get(), &reader);
        if (FAILED(hr))
            return false;

        // Set output media type to uncompressed RGB32
        ComPtr<IMFMediaType> mediaType;
        hr = MFCreateMediaType(&mediaType);
        if (FAILED(hr))
            return false;

        mediaType->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
        mediaType->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);

        hr = reader->SetCurrentMediaType(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), nullptr, mediaType.Get());
        if (FAILED(hr))
            return false;

        // Deselect all audio streams to speed up video decoding
        reader->SetStreamSelection(static_cast<DWORD>(MF_SOURCE_READER_ALL_STREAMS), FALSE);
        reader->SetStreamSelection(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), TRUE);

        // Get actual media type attributes (width, height, stride)
        ComPtr<IMFMediaType> currentType;
        hr = reader->GetCurrentMediaType(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), &currentType);
        if (FAILED(hr))
            return false;

        UINT32 width = 0, height = 0;
        hr = MFGetAttributeSize(currentType.Get(), MF_MT_FRAME_SIZE, &width, &height);
        if (FAILED(hr) || width == 0 || height == 0)
            return false;

        videoWidth = static_cast<int>(width);
        videoHeight = static_cast<int>(height);

        // Check stride
        stride = 0;
        hr = currentType->GetUINT32(MF_MT_DEFAULT_STRIDE, reinterpret_cast<UINT32*>(&stride));
        if (FAILED(hr))
        {
            hr = MFGetStrideForBitmapInfoHeader(MFVideoFormat_RGB32.Data1, width, &stride);
        }
        isBottomUp = (stride < 0);
        if (stride < 0)
            stride = -stride;

        // Get video duration
        PROPVARIANT varDuration;
        PropVariantInit(&varDuration);
        hr = reader->GetPresentationAttribute(static_cast<DWORD>(MF_SOURCE_READER_MEDIASOURCE), MF_PD_DURATION, &varDuration);
        if (SUCCEEDED(hr) && varDuration.vt == VT_UI8)
        {
            // Duration is in 100-nanosecond units
            durationSeconds = static_cast<double>(varDuration.uhVal.QuadPart) / 10'000'000.0;
        }
        PropVariantClear(&varDuration);

        currentFile = file;
        sourceReader = reader;
        return true;
    }

    juce::Image decodeFrameAt(double timeInSeconds)
    {
        const juce::ScopedLock sl(lock);
        if (!sourceReader || videoWidth <= 0 || videoHeight <= 0)
            return {};

        // Clamp target time
        const double clampedTime = juce::jlimit(0.0, juce::jmax(0.0, durationSeconds), timeInSeconds);

        // If requested time is very close to cached time, return cached image directly
        if (cachedImage.isValid() && std::abs(clampedTime - cachedTime) < 0.015) // ~1 frame at 60fps
            return cachedImage;

        // Convert seconds to 100-nanosecond units for Media Foundation seek
        LONGLONG seekPos100Ns = static_cast<LONGLONG>(clampedTime * 10'000'000.0);
        PROPVARIANT varPosition;
        PropVariantInit(&varPosition);
        varPosition.vt = VT_I8;
        varPosition.hVal.QuadPart = seekPos100Ns;

        HRESULT hr = sourceReader->SetCurrentPosition(GUID_NULL, varPosition);
        PropVariantClear(&varPosition);
        if (FAILED(hr))
            return cachedImage;

        // Read sample
        DWORD streamIndex = 0;
        DWORD flags = 0;
        LONGLONG timeStamp = 0;
        ComPtr<IMFSample> sample;

        // Try reading up to 5 times in case of stream ticks
        for (int attempts = 0; attempts < 5; ++attempts)
        {
            hr = sourceReader->ReadSample(
                static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM),
                0,
                &streamIndex,
                &flags,
                &timeStamp,
                &sample);

            if (FAILED(hr) || (flags & MF_SOURCE_READERF_ENDOFSTREAM))
                break;

            if (sample != nullptr)
                break;
        }

        if (!sample)
            return cachedImage;

        ComPtr<IMFMediaBuffer> mediaBuffer;
        hr = sample->ConvertToContiguousBuffer(&mediaBuffer);
        if (FAILED(hr) || !mediaBuffer)
            return cachedImage;

        BYTE* bufferData = nullptr;
        DWORD maxLength = 0, currentLength = 0;
        hr = mediaBuffer->Lock(&bufferData, &maxLength, &currentLength);
        if (FAILED(hr) || bufferData == nullptr)
            return cachedImage;

        juce::Image image(juce::Image::ARGB, videoWidth, videoHeight, false);
        juce::Image::BitmapData destBitmap(image, juce::Image::BitmapData::writeOnly);

        const int rowBytes = (stride > 0) ? stride : (videoWidth * 4);

        for (int y = 0; y < videoHeight; ++y)
        {
            const int srcY = isBottomUp ? (videoHeight - 1 - y) : y;
            const BYTE* srcRow = bufferData + (srcY * rowBytes);

            for (int x = 0; x < videoWidth; ++x)
            {
                // RGB32 in Media Foundation is stored as BGRA / BGR0
                const BYTE b = srcRow[x * 4 + 0];
                const BYTE g = srcRow[x * 4 + 1];
                const BYTE r = srcRow[x * 4 + 2];
                destBitmap.setPixelColour(x, y, juce::Colour(r, g, b));
            }
        }

        mediaBuffer->Unlock();

        cachedImage = image;
        cachedTime = clampedTime;
        return image;
    }
};

MFVideoDecoder::MFVideoDecoder()
    : pimpl(std::make_unique<Impl>())
{
}

MFVideoDecoder::~MFVideoDecoder() = default;

bool MFVideoDecoder::open(const juce::File& videoFile)
{
    return pimpl->open(videoFile);
}

void MFVideoDecoder::close()
{
    pimpl->close();
}

bool MFVideoDecoder::isOpen() const noexcept
{
    return pimpl->sourceReader != nullptr;
}

double MFVideoDecoder::getDurationSeconds() const noexcept
{
    return pimpl->durationSeconds;
}

int MFVideoDecoder::getWidth() const noexcept
{
    return pimpl->videoWidth;
}

int MFVideoDecoder::getHeight() const noexcept
{
    return pimpl->videoHeight;
}

const juce::File& MFVideoDecoder::getCurrentFile() const noexcept
{
    return pimpl->currentFile;
}

juce::Image MFVideoDecoder::decodeFrameAt(double timeInSeconds)
{
    return pimpl->decodeFrameAt(timeInSeconds);
}
} // namespace dawvr
