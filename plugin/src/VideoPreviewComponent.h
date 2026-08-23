// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "ServiceClient.h"

#include <juce_gui_basics/juce_gui_basics.h>

namespace dawvr
{
class VideoPreviewComponent final : public juce::Component, private juce::Timer
{
public:
    VideoPreviewComponent();
    ~VideoPreviewComponent() override;

    void paint(juce::Graphics& g) override;
    void resized() override;
    void visibilityChanged() override;

    void updateState(ServiceConnectionState connectionState,
                     const juce::String& statusText,
                     bool isArmed,
                     bool isRecording,
                     std::int64_t recordingDurationMs);

    void setFrameData(const ServiceClient::PreviewFrameData& frame);

    [[nodiscard]] bool isPreviewStreamingRequested() const noexcept;

private:
    void timerCallback() override;
    void renderPlaceholder(juce::Graphics& g, const juce::Rectangle<float>& targetBounds);
    void renderOverlay(juce::Graphics& g, const juce::Rectangle<float>& targetBounds);

    mutable juce::CriticalSection imageLock;
    juce::Image currentImage;
    std::uint64_t lastFrameRevision{};
    std::uint32_t frameWidth{};
    std::uint32_t height{};

    ServiceConnectionState currentConnectionState{ ServiceConnectionState::stopped };
    juce::String currentStatusText;
    bool armed{ false };
    bool recording{ false };
    std::int64_t durationMs{ 0 };

    bool recBlinkState{ true };
    int frameCountForFps{ 0 };
    double currentFps{ 0.0 };
    juce::int64 lastFpsCalculationTimeMs{ 0 };

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(VideoPreviewComponent)
};
} // namespace dawvr
