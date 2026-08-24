// SPDX-License-Identifier: AGPL-3.0-only
#include "RecordedPlaybackWorker.h"

#include <cmath>

namespace dawvr
{
RecordedPlaybackWorker::RecordedPlaybackWorker()
    : juce::Thread("DAW Video Recorded Playback")
{
    startThread(juce::Thread::Priority::low);
}

RecordedPlaybackWorker::~RecordedPlaybackWorker()
{
    signalThreadShouldExit();
    requestEvent.signal();
    stopThread(10'000);
}

void RecordedPlaybackWorker::requestFrame(const juce::String& videoPath, double timeSeconds)
{
    if (videoPath.isEmpty() || !std::isfinite(timeSeconds))
    {
        stopPlayback();
        return;
    }

    {
        const juce::ScopedLock lock(requestLock);
        if (latestRequest.active && videoPath == lastQueuedVideoPath
            && std::isfinite(lastQueuedTimeSeconds)
            && std::abs(timeSeconds - lastQueuedTimeSeconds) < minimumRequestStepSeconds)
        {
            return;
        }

        latestRequest.videoPath = videoPath;
        latestRequest.timeSeconds = timeSeconds;
        latestRequest.active = true;
        latestRequest.revision = ++requestRevisionCounter;
        lastQueuedVideoPath = videoPath;
        lastQueuedTimeSeconds = timeSeconds;
    }
    requestEvent.signal();
}

void RecordedPlaybackWorker::stopPlayback()
{
    {
        const juce::ScopedLock lock(requestLock);
        if (!latestRequest.active)
            return;

        latestRequest = {};
        latestRequest.revision = ++requestRevisionCounter;
        lastQueuedVideoPath.clear();
        lastQueuedTimeSeconds = std::numeric_limits<double>::quiet_NaN();
    }
    requestEvent.signal();
}

RecordedPlaybackWorker::Snapshot RecordedPlaybackWorker::getLatestSnapshot() const
{
    const juce::ScopedLock lock(snapshotLock);
    return latestSnapshot;
}

void RecordedPlaybackWorker::run()
{
    std::uint64_t handledRevision{};
    juce::String openedVideoPath;

    while (!threadShouldExit())
    {
        requestEvent.wait(100);
        if (threadShouldExit())
            break;

        Request request;
        {
            const juce::ScopedLock lock(requestLock);
            if (latestRequest.revision == handledRevision)
                continue;
            request = latestRequest;
            handledRevision = request.revision;
        }

        if (!request.active)
        {
            decoder.close();
            openedVideoPath.clear();
            publish(Status::idle, request);
            continue;
        }

        if (openedVideoPath != request.videoPath)
        {
            decoder.close();
            openedVideoPath = request.videoPath;
            publish(Status::loading, request);
            if (!decoder.open(juce::File(request.videoPath)))
            {
                publish(Status::error, request, {}, decoder.getLastError());
                continue;
            }
        }
        else if (!decoder.isOpen())
        {
            publish(Status::error, request, {}, decoder.getLastError());
            continue;
        }

        const auto frame = decoder.decodeFrameAt(request.timeSeconds);
        if (frame.isValid())
            publish(Status::ready, request, frame);
        else
            publish(Status::error, request, {}, decoder.getLastError());
    }

    decoder.close();
}

void RecordedPlaybackWorker::publish(Status status,
                                     const Request& request,
                                     const juce::Image& image,
                                     const juce::String& errorText)
{
    const juce::ScopedLock lock(snapshotLock);
    latestSnapshot.status = status;
    latestSnapshot.videoPath = request.videoPath;
    latestSnapshot.image = image;
    latestSnapshot.errorText = errorText;
    latestSnapshot.requestedTimeSeconds = request.timeSeconds;
    latestSnapshot.decodedTimeSeconds = decoder.getLastDecodedTimeSeconds();
    latestSnapshot.durationSeconds = decoder.getDurationSeconds();
    ++latestSnapshot.revision;
}
} // namespace dawvr
