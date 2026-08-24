// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "ServiceClient.h"
#include "VideoPlaybackPolicy.h"

#include <juce_gui_basics/juce_gui_basics.h>

namespace dawvr
{
class VideoPreviewComponent final : public juce::Component, private juce::Timer
{
public:
    enum class RecordedPlaybackStatus
    {
        noTake,
        positionUnavailable,
        outOfRange,
        decoderError,
        ready
    };

    struct PlaybackState final
    {
        VideoPresentationSource presentationSource{ VideoPresentationSource::liveCamera };
        RecordedPlaybackStatus recordedStatus{ RecordedPlaybackStatus::noTake };
        bool isDawPlaying{ false };
        bool isDawRecording{ false };
        bool isArmed{ false };
        bool hasTake{ false };
        juce::String takeLabel;
        double currentVideoTimeSec{ 0.0 };
        double totalDurationSec{ 0.0 };
        std::int64_t currentDawSample{ 0 };
    };

    VideoPreviewComponent();
    ~VideoPreviewComponent() override;

    void paint(juce::Graphics& g) override;
    void resized() override;
    void visibilityChanged() override;

    void setViewMode(VideoViewMode mode);
    [[nodiscard]] VideoViewMode getViewMode() const noexcept { return viewMode; }

    void updateServiceState(ServiceConnectionState connectionState,
                            const juce::String& statusText,
                            bool isArmed,
                            bool isRecording,
                            std::int64_t recordingDurationMs);

    void updatePlaybackState(const PlaybackState& state);

    void setLiveFrameData(const ServiceClient::PreviewFrameData& frame);
    void setRecordedFrame(const juce::Image& image);
    void clearRecordedFrame();

    [[nodiscard]] bool isPreviewStreamingRequested() const noexcept;

private:
    void timerCallback() override;
    void renderPlaceholder(juce::Graphics& g, const juce::Rectangle<float>& targetBounds);
    void renderOverlay(juce::Graphics& g, const juce::Rectangle<float>& videoBounds);

    VideoViewMode viewMode{ VideoViewMode::autoSync };

    mutable juce::CriticalSection imageLock;
    juce::Image currentLiveImage;
    juce::Image currentRecordedImage;
    std::uint64_t lastLiveFrameRevision{};
    std::uint32_t liveFrameWidth{};
    std::uint32_t liveFrameHeight{};

    ServiceConnectionState currentConnectionState{ ServiceConnectionState::stopped };
    juce::String currentStatusText;
    bool armed{ false };
    bool recording{ false };
    std::int64_t durationMs{ 0 };

    PlaybackState playbackState;

    bool recBlinkState{ true };
    int frameCountForFps{ 0 };
    double currentFps{ 0.0 };
    juce::int64 lastFpsCalculationTimeMs{ 0 };

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(VideoPreviewComponent)
};
} // namespace dawvr
