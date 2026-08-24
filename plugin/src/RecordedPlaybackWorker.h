// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "MFVideoDecoder.h"

#include <juce_core/juce_core.h>
#include <juce_graphics/juce_graphics.h>

#include <cstdint>
#include <limits>

namespace dawvr
{
class RecordedPlaybackWorker final : private juce::Thread
{
public:
    enum class Status
    {
        idle,
        loading,
        ready,
        error
    };

    struct Snapshot final
    {
        Status status{ Status::idle };
        juce::String videoPath;
        juce::Image image;
        juce::String errorText;
        double requestedTimeSeconds{ 0.0 };
        double decodedTimeSeconds{ 0.0 };
        double durationSeconds{ 0.0 };
        std::uint64_t revision{ 0 };
    };

    RecordedPlaybackWorker();
    ~RecordedPlaybackWorker() override;

    // Non-blocking. Requests are capped at 15 fps and coalesced to the newest target.
    void requestFrame(const juce::String& videoPath, double timeSeconds);
    void stopPlayback();
    [[nodiscard]] Snapshot getLatestSnapshot() const;

private:
    struct Request final
    {
        juce::String videoPath;
        double timeSeconds{ 0.0 };
        std::uint64_t revision{ 0 };
        bool active{ false };
    };

    void run() override;
    void publish(Status status,
                 const Request& request,
                 const juce::Image& image = {},
                 const juce::String& errorText = {});

    static constexpr double minimumRequestStepSeconds = 1.0 / 15.0;

    mutable juce::CriticalSection requestLock;
    Request latestRequest;
    std::uint64_t requestRevisionCounter{};
    double lastQueuedTimeSeconds{ std::numeric_limits<double>::quiet_NaN() };
    juce::String lastQueuedVideoPath;
    juce::WaitableEvent requestEvent;

    mutable juce::CriticalSection snapshotLock;
    Snapshot latestSnapshot;

    MFVideoDecoder decoder;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(RecordedPlaybackWorker)
};
} // namespace dawvr
