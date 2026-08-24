// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include <cstdint>
#include <optional>

namespace dawvr
{
enum class VideoViewMode
{
    autoSync = 0,
    recordedTake = 1,
    liveCamera = 2
};

enum class VideoPresentationSource
{
    liveCamera,
    recordedTake
};

struct VideoPresentationContext final
{
    VideoViewMode viewMode{ VideoViewMode::autoSync };
    bool isDawPlaying{ false };
    bool isDawRecording{ false };
    bool isArmed{ false };
    bool hasTake{ false };
};

[[nodiscard]] constexpr VideoPresentationSource resolveVideoPresentationSource(
    const VideoPresentationContext& context) noexcept
{
    if (context.viewMode == VideoViewMode::liveCamera)
        return VideoPresentationSource::liveCamera;

    if (context.viewMode == VideoViewMode::recordedTake)
        return VideoPresentationSource::recordedTake;

    if (context.isDawRecording)
        return VideoPresentationSource::liveCamera;

    if (context.isDawPlaying && context.hasTake)
        return VideoPresentationSource::recordedTake;

    if (context.isArmed || !context.hasTake)
        return VideoPresentationSource::liveCamera;

    return VideoPresentationSource::recordedTake;
}

// syncOffsetNs is first_frame_monotonic_time_ns minus the corrected DAW start time.
// A negative value therefore means that the MP4 begins before the DAW take start.
[[nodiscard]] constexpr std::optional<double> mapProjectSampleToVideoTimeSeconds(
    std::int64_t currentProjectSample,
    std::int64_t startProjectSample,
    double sampleRate,
    std::int64_t syncOffsetNs) noexcept
{
    if (sampleRate <= 0.0)
        return std::nullopt;

    const auto sampleDelta = currentProjectSample - startProjectSample;
    const auto dawElapsedSeconds = static_cast<double>(sampleDelta) / sampleRate;
    const auto syncOffsetSeconds = static_cast<double>(syncOffsetNs) / 1'000'000'000.0;
    return dawElapsedSeconds - syncOffsetSeconds;
}
} // namespace dawvr
