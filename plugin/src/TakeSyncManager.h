// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "VideoPlaybackPolicy.h"

#include <juce_core/juce_core.h>

#include <cstdint>
#include <optional>
#include <vector>

namespace dawvr
{
struct TakeSyncInfo final
{
    juce::String takeId;
    juce::String persistentInstanceId;
    juce::String instanceLabel;
    juce::String sourceDisplayName;
    juce::String videoPath;
    juce::String metadataPath;
    juce::String recordStartedAt;
    std::int64_t startProjectSample{ 0 };
    std::int64_t endProjectSample{ 0 };
    double sampleRate{ 48000.0 };
    std::int64_t syncOffsetNs{ 0 };
    std::uint64_t frameCount{ 0 };
};

class TakeSyncManager final
{
public:
    TakeSyncManager() = default;
    ~TakeSyncManager() = default;

    // Rescans the <projectRoot>/Recordings directory for current .sync.json files
    // and legacy .mp4.json files.
    // Returns true if the take list has changed.
    bool refreshTakes(const juce::String& projectRoot, const juce::String& persistentInstanceId);

    // Explicitly add or update a finalized take
    void addFinalizedTake(const juce::String& metadataPath);

    [[nodiscard]] std::vector<TakeSyncInfo> getAvailableTakes() const;
    [[nodiscard]] std::optional<TakeSyncInfo> getTakeById(const juce::String& takeId) const;
    [[nodiscard]] std::optional<TakeSyncInfo> getSelectedTake() const;

    void setSelectedTakeId(const juce::String& takeId);
    [[nodiscard]] juce::String getSelectedTakeId() const;

    // Returns whether currentSample is within the range of the currently selected take
    [[nodiscard]] bool isSampleInRange(std::int64_t currentProjectSample) const;

    // Calculates time since the first MP4 frame for the selected DAW sample position.
    // syncOffsetNs is subtracted because it is first-frame time minus corrected DAW-start time.
    // Returns std::nullopt if no take is selected or current sample is invalid.
    [[nodiscard]] std::optional<double> calculateVideoTimeSeconds(std::int64_t currentProjectSample) const;

    [[nodiscard]] std::uint64_t getRevision() const;

private:
    static std::optional<TakeSyncInfo> parseSyncJson(const juce::File& jsonFile);

    mutable juce::CriticalSection lock;
    std::vector<TakeSyncInfo> availableTakes;
    juce::String selectedTakeId;
    juce::String persistentInstanceIdFilter;
    std::uint64_t revision{ 0 };

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(TakeSyncManager)
};
} // namespace dawvr
