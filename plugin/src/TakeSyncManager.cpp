// SPDX-License-Identifier: AGPL-3.0-only
#include "TakeSyncManager.h"

#include <algorithm>

namespace dawvr
{
std::optional<TakeSyncInfo> TakeSyncManager::parseSyncJson(const juce::File& jsonFile)
{
    if (!jsonFile.existsAsFile())
        return std::nullopt;

    const auto jsonString = jsonFile.loadFileAsString();
    const auto var = juce::JSON::parse(jsonString);
    if (!var.isObject())
        return std::nullopt;

    const auto* obj = var.getDynamicObject();
    if (obj == nullptr)
        return std::nullopt;

    TakeSyncInfo info;
    info.metadataPath = jsonFile.getFullPathName();
    info.takeId = obj->getProperty("take_id").toString();
    info.persistentInstanceId = obj->getProperty("persistent_instance_id").toString();
    info.instanceLabel = obj->getProperty("instance_label").toString();
    info.sourceDisplayName = obj->getProperty("source_display_name").toString();
    info.recordStartedAt = obj->getProperty("record_started_at").toString();

    auto rawVideoPath = obj->getProperty("output_video_path").toString();
    if (juce::File::isAbsolutePath(rawVideoPath))
    {
        info.videoPath = rawVideoPath;
    }
    else
    {
        info.videoPath = jsonFile.getParentDirectory().getChildFile(rawVideoPath).getFullPathName();
    }

    if (obj->hasProperty("start_project_sample"))
        info.startProjectSample = static_cast<std::int64_t>(obj->getProperty("start_project_sample"));

    if (obj->hasProperty("end_project_sample"))
        info.endProjectSample = static_cast<std::int64_t>(obj->getProperty("end_project_sample"));

    if (obj->hasProperty("sample_rate"))
        info.sampleRate = static_cast<double>(obj->getProperty("sample_rate"));

    if (info.sampleRate <= 0.0)
        info.sampleRate = 48000.0;

    if (obj->hasProperty("sync_offset_ns"))
        info.syncOffsetNs = static_cast<std::int64_t>(obj->getProperty("sync_offset_ns"));

    if (obj->hasProperty("frame_count"))
        info.frameCount = static_cast<std::uint64_t>(static_cast<std::int64_t>(obj->getProperty("frame_count")));

    return info;
}

bool TakeSyncManager::refreshTakes(const juce::String& projectRoot, const juce::String& persistentInstanceId)
{
    const juce::ScopedLock sl(lock);
    persistentInstanceIdFilter = persistentInstanceId;

    if (projectRoot.isEmpty())
    {
        if (!availableTakes.empty())
        {
            availableTakes.clear();
            selectedTakeId.clear();
            ++revision;
            return true;
        }
        return false;
    }

    const juce::File recordingsDir = juce::File(projectRoot).getChildFile("Recordings");
    if (!recordingsDir.isDirectory())
    {
        if (!availableTakes.empty())
        {
            availableTakes.clear();
            selectedTakeId.clear();
            ++revision;
            return true;
        }
        return false;
    }

    juce::Array<juce::File> jsonFiles;
    recordingsDir.findChildFiles(jsonFiles, juce::File::findFiles, true, "*.sync.json");
    recordingsDir.findChildFiles(jsonFiles, juce::File::findFiles, true, "*.mp4.json");

    std::vector<TakeSyncInfo> newTakes;
    for (const auto& file : jsonFiles)
    {
        if (auto take = parseSyncJson(file))
        {
            // Only include takes matching this plugin instance (or all if ID is empty)
            if (persistentInstanceId.isEmpty() || take->persistentInstanceId == persistentInstanceId)
            {
                // Verify corresponding video file exists
                if (juce::File(take->videoPath).existsAsFile())
                {
                    const auto duplicate = std::find_if(
                        newTakes.begin(), newTakes.end(), [&take](const TakeSyncInfo& existing) {
                            return existing.takeId == take->takeId;
                        });
                    if (duplicate == newTakes.end())
                        newTakes.push_back(std::move(*take));
                }
            }
        }
    }

    // Sort takes by recordStartedAt descending (latest first)
    std::sort(newTakes.begin(), newTakes.end(), [](const TakeSyncInfo& a, const TakeSyncInfo& b) {
        return a.recordStartedAt > b.recordStartedAt;
    });

    bool changed = false;
    if (newTakes.size() != availableTakes.size())
    {
        changed = true;
    }
    else
    {
        for (std::size_t i = 0; i < newTakes.size(); ++i)
        {
            if (newTakes[i].takeId != availableTakes[i].takeId ||
                newTakes[i].videoPath != availableTakes[i].videoPath)
            {
                changed = true;
                break;
            }
        }
    }

    if (changed)
    {
        availableTakes = std::move(newTakes);
        if (!availableTakes.empty())
        {
            // If current selectedTakeId is not in availableTakes, select the latest one
            const auto it = std::find_if(availableTakes.begin(), availableTakes.end(),
                                         [this](const TakeSyncInfo& t) { return t.takeId == selectedTakeId; });
            if (it == availableTakes.end())
            {
                selectedTakeId = availableTakes.front().takeId;
            }
        }
        else
        {
            selectedTakeId.clear();
        }
        ++revision;
    }

    return changed;
}

void TakeSyncManager::addFinalizedTake(const juce::String& metadataPath)
{
    const juce::ScopedLock sl(lock);
    const juce::File file(metadataPath);
    if (auto take = parseSyncJson(file))
    {
        if ((!persistentInstanceIdFilter.isEmpty()
             && take->persistentInstanceId != persistentInstanceIdFilter)
            || !juce::File(take->videoPath).existsAsFile())
        {
            return;
        }

        const auto it = std::find_if(availableTakes.begin(), availableTakes.end(),
                                     [&take](const TakeSyncInfo& t) { return t.takeId == take->takeId; });
        if (it != availableTakes.end())
        {
            *it = *take;
        }
        else
        {
            availableTakes.insert(availableTakes.begin(), *take);
        }
        selectedTakeId = take->takeId;
        ++revision;
    }
}

std::vector<TakeSyncInfo> TakeSyncManager::getAvailableTakes() const
{
    const juce::ScopedLock sl(lock);
    return availableTakes;
}

std::optional<TakeSyncInfo> TakeSyncManager::getTakeById(const juce::String& takeId) const
{
    const juce::ScopedLock sl(lock);
    for (const auto& take : availableTakes)
    {
        if (take.takeId == takeId)
            return take;
    }
    return std::nullopt;
}

std::optional<TakeSyncInfo> TakeSyncManager::getSelectedTake() const
{
    const juce::ScopedLock sl(lock);
    for (const auto& take : availableTakes)
    {
        if (take.takeId == selectedTakeId)
            return take;
    }
    return std::nullopt;
}

void TakeSyncManager::setSelectedTakeId(const juce::String& takeId)
{
    const juce::ScopedLock sl(lock);
    if (selectedTakeId != takeId)
    {
        selectedTakeId = takeId;
        ++revision;
    }
}

juce::String TakeSyncManager::getSelectedTakeId() const
{
    const juce::ScopedLock sl(lock);
    return selectedTakeId;
}

bool TakeSyncManager::isSampleInRange(std::int64_t currentProjectSample) const
{
    const auto take = getSelectedTake();
    if (!take.has_value())
        return false;

    return (currentProjectSample >= take->startProjectSample &&
            currentProjectSample <= take->endProjectSample);
}

std::optional<double> TakeSyncManager::calculateVideoTimeSeconds(std::int64_t currentProjectSample) const
{
    const auto take = getSelectedTake();
    if (!take.has_value() || take->sampleRate <= 0.0)
        return std::nullopt;

    const auto sampleDelta = currentProjectSample - take->startProjectSample;
    const double rawSeconds = static_cast<double>(sampleDelta) / take->sampleRate;
    const double offsetSeconds = static_cast<double>(take->syncOffsetNs) / 1'000'000'000.0;

    return rawSeconds + offsetSeconds;
}

std::uint64_t TakeSyncManager::getRevision() const
{
    const juce::ScopedLock sl(lock);
    return revision;
}
} // namespace dawvr
