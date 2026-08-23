// SPDX-License-Identifier: AGPL-3.0-only
#include "VideoPreviewComponent.h"

namespace dawvr
{
VideoPreviewComponent::VideoPreviewComponent()
{
    startTimerHz(30);
}

VideoPreviewComponent::~VideoPreviewComponent()
{
    stopTimer();
}

void VideoPreviewComponent::paint(juce::Graphics& g)
{
    const auto bounds = getLocalBounds().toFloat();
    g.fillAll(juce::Colour(0xff121214)); // Dark background

    juce::Image imageToDraw;
    {
        const juce::ScopedLock lock(imageLock);
        if (currentImage.isValid())
            imageToDraw = currentImage;
    }

    if (imageToDraw.isValid())
    {
        const auto imgW = static_cast<float>(imageToDraw.getWidth());
        const auto imgH = static_cast<float>(imageToDraw.getHeight());
        const auto scale = juce::jmin(bounds.getWidth() / imgW, bounds.getHeight() / imgH);
        const auto targetW = imgW * scale;
        const auto targetH = imgH * scale;
        const auto targetX = bounds.getX() + (bounds.getWidth() - targetW) * 0.5f;
        const auto targetY = bounds.getY() + (bounds.getHeight() - targetH) * 0.5f;
        const juce::Rectangle<float> videoBounds(targetX, targetY, targetW, targetH);

        g.drawImage(imageToDraw,
                    videoBounds,
                    juce::RectanglePlacement::centred | juce::RectanglePlacement::fillDestination);

        // Border around video
        g.setColour(juce::Colour(0xff333338));
        g.drawRect(videoBounds, 1.0f);

        renderOverlay(g, videoBounds);
    }
    else
    {
        renderPlaceholder(g, bounds);
    }

    // Outer frame border
    g.setColour(juce::Colour(0xff2a2a30));
    g.drawRect(bounds, 1.0f);
}

void VideoPreviewComponent::resized()
{
}

void VideoPreviewComponent::visibilityChanged()
{
}

bool VideoPreviewComponent::isPreviewStreamingRequested() const noexcept
{
    return isShowing();
}

void VideoPreviewComponent::updateState(ServiceConnectionState connectionState,
                                        const juce::String& statusText,
                                        bool isArmed,
                                        bool isRecording,
                                        std::int64_t recordingDurationMs)
{
    currentConnectionState = connectionState;
    currentStatusText = statusText;
    armed = isArmed;
    recording = isRecording;
    durationMs = recordingDurationMs;
}

void VideoPreviewComponent::setFrameData(const ServiceClient::PreviewFrameData& frame)
{
    if (frame.revision == lastFrameRevision || frame.frameData.empty())
        return;

    lastFrameRevision = frame.revision;
    frameWidth = frame.width;
    height = frame.height;
    ++frameCountForFps;

    juce::Image decoded;
    if (frame.format == "jpeg")
    {
        juce::MemoryInputStream stream(frame.frameData.data(), frame.frameData.size(), false);
        juce::JPEGImageFormat jpegReader;
        decoded = jpegReader.decodeImage(stream);
    }
    else if (frame.format == "rgb24" && frame.width > 0 && frame.height > 0)
    {
        decoded = juce::Image(juce::Image::RGB,
                              static_cast<int>(frame.width),
                              static_cast<int>(frame.height),
                              false);
        juce::Image::BitmapData destData(decoded, juce::Image::BitmapData::writeOnly);
        const auto expectedSize = static_cast<std::size_t>(frame.width) * frame.height * 3U;
        if (frame.frameData.size() >= expectedSize)
        {
            const auto* srcPtr = frame.frameData.data();
            for (int y = 0; y < static_cast<int>(frame.height); ++y)
            {
                for (int x = 0; x < static_cast<int>(frame.width); ++x)
                {
                    const auto r = *srcPtr++;
                    const auto g = *srcPtr++;
                    const auto b = *srcPtr++;
                    destData.setPixelColour(x, y, juce::Colour(r, g, b));
                }
            }
        }
    }

    if (decoded.isValid())
    {
        const juce::ScopedLock lock(imageLock);
        currentImage = std::move(decoded);
    }
}

void VideoPreviewComponent::timerCallback()
{
    const auto now = juce::Time::currentTimeMillis();
    if (lastFpsCalculationTimeMs == 0)
    {
        lastFpsCalculationTimeMs = now;
    }
    else if (now - lastFpsCalculationTimeMs >= 1000)
    {
        currentFps = static_cast<double>(frameCountForFps) * 1000.0
                     / static_cast<double>(now - lastFpsCalculationTimeMs);
        frameCountForFps = 0;
        lastFpsCalculationTimeMs = now;
    }

    // Toggle REC blinking approximately twice per second
    if ((now / 500) % 2 == 0)
        recBlinkState = true;
    else
        recBlinkState = false;

    repaint();
}

void VideoPreviewComponent::renderPlaceholder(juce::Graphics& g, const juce::Rectangle<float>& targetBounds)
{
    g.setColour(juce::Colour(0xff1e1e24));
    g.fillRect(targetBounds.reduced(2.0f));

    g.setColour(juce::Colour(0xff777788));
    g.setFont(juce::FontOptions(14.0f, juce::Font::plain));

    juce::String placeholderText;
    if (currentConnectionState != ServiceConnectionState::connected)
    {
        placeholderText = "Recording Service Not Connected\n(" + currentStatusText + ")";
    }
    else
    {
        placeholderText = "No Video Preview Available\nSelect a camera source to start preview";
    }

    g.drawFittedText(placeholderText,
                     targetBounds.toNearestInt(),
                     juce::Justification::centred,
                     3);
}

void VideoPreviewComponent::renderOverlay(juce::Graphics& g, const juce::Rectangle<float>& videoBounds)
{
    // Status overlay (top-left)
    if (recording)
    {
        const juce::Rectangle<float> badgeBounds(videoBounds.getX() + 10.0f, videoBounds.getY() + 10.0f, 150.0f, 26.0f);
        g.setColour(juce::Colour(0xcc1a1a1e));
        g.fillRoundedRectangle(badgeBounds, 4.0f);

        // Blinking red dot
        if (recBlinkState)
        {
            g.setColour(juce::Colour(0xffe53935));
            g.fillEllipse(badgeBounds.getX() + 8.0f, badgeBounds.getY() + 7.0f, 12.0f, 12.0f);
        }

        g.setColour(juce::Colours::white);
        g.setFont(juce::FontOptions(12.0f, juce::Font::bold));

        const auto totalSeconds = durationMs / 1000;
        const auto hours = totalSeconds / 3600;
        const auto minutes = (totalSeconds % 3600) / 60;
        const auto seconds = totalSeconds % 60;
        const auto millis = durationMs % 1000;

        const auto timecode = juce::String::formatted("REC %02d:%02d:%02d.%03d",
                                                      static_cast<int>(hours),
                                                      static_cast<int>(minutes),
                                                      static_cast<int>(seconds),
                                                      static_cast<int>(millis));

        g.drawText(timecode,
                   badgeBounds.withTrimmedLeft(26.0f).toNearestInt(),
                   juce::Justification::centredLeft,
                   false);
    }
    else if (armed)
    {
        const juce::Rectangle<float> badgeBounds(videoBounds.getX() + 10.0f, videoBounds.getY() + 10.0f, 80.0f, 24.0f);
        g.setColour(juce::Colour(0xccb71c1c));
        g.fillRoundedRectangle(badgeBounds, 4.0f);

        g.setColour(juce::Colours::white);
        g.setFont(juce::FontOptions(11.0f, juce::Font::bold));
        g.drawText("ARMED", badgeBounds.toNearestInt(), juce::Justification::centred, false);
    }

    // FPS overlay (bottom-right)
    if (currentFps > 0.1)
    {
        const juce::Rectangle<float> fpsBounds(videoBounds.getRight() - 90.0f, videoBounds.getBottom() - 28.0f, 80.0f, 20.0f);
        g.setColour(juce::Colour(0x99101014));
        g.fillRoundedRectangle(fpsBounds, 3.0f);

        g.setColour(juce::Colour(0xffbbbbcc));
        g.setFont(juce::FontOptions(11.0f, juce::Font::plain));
        g.drawText(juce::String::formatted("%.1f fps", currentFps),
                   fpsBounds.toNearestInt(),
                   juce::Justification::centred,
                   false);
    }
}
} // namespace dawvr
