// SPDX-License-Identifier: AGPL-3.0-only
#include "MFVideoDecoder.h"

#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <mferror.h>
#include <propvarutil.h>
#include <wrl/client.h>

#include <algorithm>
#include <cmath>

namespace dawvr
{
using Microsoft::WRL::ComPtr;

struct MFVideoDecoder::Impl
{
    juce::CriticalSection lock;
    juce::File currentFile;
    ComPtr<IMFSourceReader> sourceReader;
    int sourceFrameWidth{ 0 };
    int sourceFrameHeight{ 0 };
    int videoWidth{ 0 };
    int videoHeight{ 0 };
    double durationSeconds{ 0.0 };
    bool isBottomUp{ false };
    LONG stride{ 0 };

    juce::Image cachedImage;
    double cachedTime{ -1.0 };
    double lastDecodedTimeSeconds{ -1.0 };
    bool mfInitialized{ false };
    juce::String lastError;

    Impl()
    {
        HRESULT hr = MFStartup(MF_VERSION, MFSTARTUP_FULL);
        if (SUCCEEDED(hr))
            mfInitialized = true;
        else
            lastError = "MFStartup failed (HRESULT " + juce::String::toHexString(hr) + ")";
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
        sourceFrameWidth = 0;
        sourceFrameHeight = 0;
        videoWidth = 0;
        videoHeight = 0;
        durationSeconds = 0.0;
        cachedImage = {};
        cachedTime = -1.0;
        lastDecodedTimeSeconds = -1.0;
    }

    bool fail(const char* operation, HRESULT result = S_OK)
    {
        lastError = operation;
        if (FAILED(result))
            lastError += " (HRESULT " + juce::String::toHexString(result) + ")";
        return false;
    }

    bool open(const juce::File& file)
    {
        const juce::ScopedLock sl(lock);
        close();
        lastError.clear();

        if (!file.existsAsFile())
            return fail("Video file does not exist");
        if (!mfInitialized)
            return fail("Media Foundation is not initialized");

        ComPtr<IMFAttributes> attributes;
        HRESULT hr = MFCreateAttributes(&attributes, 2);
        if (FAILED(hr))
            return fail("MFCreateAttributes failed", hr);

        // Enable video processing and low latency reading
        attributes->SetUINT32(MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, TRUE);
        attributes->SetUINT32(MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, TRUE);

        auto sourcePath = file.getFullPathName();
        if (sourcePath.startsWith("\\\\?\\UNC\\"))
            sourcePath = "\\\\" + sourcePath.substring(8);
        else if (sourcePath.startsWith("\\\\?\\"))
            sourcePath = sourcePath.substring(4);

        ComPtr<IMFSourceReader> reader;
        hr = MFCreateSourceReaderFromURL(sourcePath.toWideCharPointer(), attributes.Get(), &reader);
        if (FAILED(hr))
            return fail("MFCreateSourceReaderFromURL failed", hr);

        // Set output media type to uncompressed RGB32
        ComPtr<IMFMediaType> mediaType;
        hr = MFCreateMediaType(&mediaType);
        if (FAILED(hr))
            return fail("MFCreateMediaType failed", hr);

        mediaType->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
        mediaType->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);

        hr = reader->SetCurrentMediaType(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), nullptr, mediaType.Get());
        if (FAILED(hr))
            return fail("SetCurrentMediaType RGB32 failed", hr);

        // Deselect all audio streams to speed up video decoding
        reader->SetStreamSelection(static_cast<DWORD>(MF_SOURCE_READER_ALL_STREAMS), FALSE);
        reader->SetStreamSelection(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), TRUE);

        // Get actual media type attributes (width, height, stride)
        ComPtr<IMFMediaType> currentType;
        hr = reader->GetCurrentMediaType(static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM), &currentType);
        if (FAILED(hr))
            return fail("GetCurrentMediaType failed", hr);

        UINT32 width = 0, height = 0;
        hr = MFGetAttributeSize(currentType.Get(), MF_MT_FRAME_SIZE, &width, &height);
        if (FAILED(hr) || width == 0 || height == 0)
            return fail("Video frame size is unavailable", hr);

        sourceFrameWidth = static_cast<int>(width);
        sourceFrameHeight = static_cast<int>(height);

        constexpr int maximumPreviewWidth = 640;
        constexpr int maximumPreviewHeight = 360;
        const auto previewScale = juce::jmin(
            1.0,
            juce::jmin(static_cast<double>(maximumPreviewWidth) / sourceFrameWidth,
                       static_cast<double>(maximumPreviewHeight) / sourceFrameHeight));
        videoWidth = juce::jmax(1, static_cast<int>(sourceFrameWidth * previewScale));
        videoHeight = juce::jmax(1, static_cast<int>(sourceFrameHeight * previewScale));

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
        lastError.clear();
        return true;
    }

    juce::Image decodeFrameAt(double timeInSeconds)
    {
        const juce::ScopedLock sl(lock);
        if (!sourceReader || sourceFrameWidth <= 0 || sourceFrameHeight <= 0
            || videoWidth <= 0 || videoHeight <= 0)
        {
            lastError = "Video decoder is not open";
            return {};
        }

        // Avoid seeking to the exact end-of-stream position, where no sample may exist.
        const auto lastDecodableTime = durationSeconds > 0.001
                                           ? durationSeconds - 0.001
                                           : juce::jmax(0.0, durationSeconds);
        const double clampedTime = juce::jlimit(0.0, lastDecodableTime, timeInSeconds);

        bool needsSeek = true;
        if (cachedImage.isValid())
        {
            const auto deltaSeconds = clampedTime - cachedTime;
            if (std::abs(deltaSeconds) < (1.0 / 60.0))
                return cachedImage;
            needsSeek = deltaSeconds <= 0.0 || deltaSeconds > 0.25;
        }

        HRESULT hr = S_OK;
        if (needsSeek)
        {
            const auto seekPos100Ns = static_cast<LONGLONG>(clampedTime * 10'000'000.0);
            PROPVARIANT varPosition;
            PropVariantInit(&varPosition);
            varPosition.vt = VT_I8;
            varPosition.hVal.QuadPart = seekPos100Ns;
            hr = sourceReader->SetCurrentPosition(GUID_NULL, varPosition);
            PropVariantClear(&varPosition);
            if (FAILED(hr))
            {
                lastError = "SetCurrentPosition failed (HRESULT "
                            + juce::String::toHexString(hr) + ")";
                return cachedImage;
            }
        }

        DWORD streamIndex = 0;
        DWORD flags = 0;
        LONGLONG timeStamp = 0;
        ComPtr<IMFSample> sample;
        const auto maximumReadAttempts = needsSeek ? 120 : 32;
        for (int attempts = 0; attempts < maximumReadAttempts; ++attempts)
        {
            sample.Reset();
            hr = sourceReader->ReadSample(
                static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM),
                0,
                &streamIndex,
                &flags,
                &timeStamp,
                &sample);

            if (FAILED(hr) || (flags & MF_SOURCE_READERF_ENDOFSTREAM))
                break;

            if (sample == nullptr)
                continue;

            const auto sampleTimeSeconds = static_cast<double>(timeStamp) / 10'000'000.0;
            if (sampleTimeSeconds >= clampedTime - (1.0 / 120.0))
                break;
        }

        if (!sample)
        {
            lastError = "No video sample was returned for the requested time";
            return cachedImage;
        }

        ComPtr<IMFMediaBuffer> mediaBuffer;
        hr = sample->ConvertToContiguousBuffer(&mediaBuffer);
        if (FAILED(hr) || !mediaBuffer)
        {
            lastError = "ConvertToContiguousBuffer failed (HRESULT "
                        + juce::String::toHexString(hr) + ")";
            return cachedImage;
        }

        BYTE* bufferData = nullptr;
        DWORD maxLength = 0, currentLength = 0;
        hr = mediaBuffer->Lock(&bufferData, &maxLength, &currentLength);
        if (FAILED(hr) || bufferData == nullptr)
        {
            lastError = "Media buffer lock failed (HRESULT "
                        + juce::String::toHexString(hr) + ")";
            return cachedImage;
        }

        juce::Image image(juce::Image::ARGB, videoWidth, videoHeight, true);
        juce::Image::BitmapData destBitmap(image, juce::Image::BitmapData::writeOnly);

        const int rowBytes = (stride > 0) ? stride : (sourceFrameWidth * 4);

        for (int y = 0; y < videoHeight; ++y)
        {
            const auto scaledSourceY = (y * sourceFrameHeight) / videoHeight;
            const int srcY = isBottomUp ? (sourceFrameHeight - 1 - scaledSourceY)
                                        : scaledSourceY;
            const BYTE* srcRow = bufferData + (srcY * rowBytes);
            auto* destRow = destBitmap.data + (y * destBitmap.lineStride);

            for (int x = 0; x < videoWidth; ++x)
            {
                const auto sourceX = (x * sourceFrameWidth) / videoWidth;
                const BYTE b = srcRow[sourceX * 4 + 0];
                const BYTE g = srcRow[sourceX * 4 + 1];
                const BYTE r = srcRow[sourceX * 4 + 2];
                auto* pixel = reinterpret_cast<juce::PixelARGB*>(
                    destRow + (x * destBitmap.pixelStride));
                pixel->setARGB(0xff, r, g, b);
            }
        }

        mediaBuffer->Unlock();

        cachedImage = image;
        cachedTime = static_cast<double>(timeStamp) / 10'000'000.0;
        lastDecodedTimeSeconds = cachedTime;
        lastError.clear();
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

double MFVideoDecoder::getLastDecodedTimeSeconds() const noexcept
{
    return pimpl->lastDecodedTimeSeconds;
}

juce::String MFVideoDecoder::getLastError() const
{
    const juce::ScopedLock sl(pimpl->lock);
    return pimpl->lastError;
}

juce::Image MFVideoDecoder::decodeFrameAt(double timeInSeconds)
{
    return pimpl->decodeFrameAt(timeInSeconds);
}
} // namespace dawvr
