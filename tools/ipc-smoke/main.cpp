// SPDX-License-Identifier: AGPL-3.0-only
#include "ServiceClient.h"
#include "MFVideoDecoder.h"
#include "VideoPlaybackPolicy.h"

#include <juce_core/juce_core.h>

#include <atomic>
#include <chrono>
#include <cmath>
#include <iostream>
#include <string_view>

namespace
{
std::int64_t monotonicTimeNs() noexcept
{
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

bool runPlaybackLogicSelfTest()
{
    using dawvr::VideoPresentationContext;
    using dawvr::VideoPresentationSource;
    using dawvr::VideoViewMode;

    const auto resolvesTo = [](VideoViewMode mode,
                               bool playing,
                               bool recording,
                               bool armed,
                               bool hasTake,
                               VideoPresentationSource expected) {
        return dawvr::resolveVideoPresentationSource(
                   VideoPresentationContext{ mode, playing, recording, armed, hasTake })
               == expected;
    };

    if (!resolvesTo(VideoViewMode::autoSync, true, false, true, true,
                    VideoPresentationSource::recordedTake)
        || !resolvesTo(VideoViewMode::autoSync, true, true, true, true,
                       VideoPresentationSource::liveCamera)
        || !resolvesTo(VideoViewMode::autoSync, false, false, true, true,
                       VideoPresentationSource::liveCamera)
        || !resolvesTo(VideoViewMode::autoSync, false, false, false, true,
                       VideoPresentationSource::recordedTake)
        || !resolvesTo(VideoViewMode::autoSync, true, false, false, false,
                       VideoPresentationSource::liveCamera)
        || !resolvesTo(VideoViewMode::recordedTake, false, false, true, false,
                       VideoPresentationSource::recordedTake)
        || !resolvesTo(VideoViewMode::liveCamera, true, false, false, true,
                       VideoPresentationSource::liveCamera))
    {
        std::cerr << "Video presentation policy self-test failed\n";
        return false;
    }

    const auto negativeOffset = dawvr::mapProjectSampleToVideoTimeSeconds(
        96'000, 96'000, 48'000.0, -1'370'774'700);
    const auto positiveOffset = dawvr::mapProjectSampleToVideoTimeSeconds(
        96'000, 96'000, 48'000.0, 250'000'000);
    const auto oneSecondLater = dawvr::mapProjectSampleToVideoTimeSeconds(
        144'000, 96'000, 48'000.0, -1'370'774'700);
    const auto invalidRate = dawvr::mapProjectSampleToVideoTimeSeconds(
        96'000, 96'000, 0.0, 0);
    if (!negativeOffset.has_value() || !positiveOffset.has_value()
        || !oneSecondLater.has_value() || invalidRate.has_value()
        || std::abs(*negativeOffset - 1.3707747) > 0.0000001
        || std::abs(*positiveOffset + 0.25) > 0.0000001
        || std::abs(*oneSecondLater - 2.3707747) > 0.0000001)
    {
        std::cerr << "Video synchronization mapping self-test failed\n";
        return false;
    }

    std::cout << "Playback policy and synchronization mapping self-test passed\n";
    return true;
}
} // namespace

int main(int argc, char** argv)
{
    if (argc == 3 && std::string_view(argv[1]) == "--decode-file")
    {
        dawvr::MFVideoDecoder decoder;
        if (!decoder.open(juce::File::createFileWithoutCheckingPath(argv[2])))
        {
            std::cerr << "Decoder open failed: " << decoder.getLastError() << '\n';
            return 31;
        }
        const auto frame = decoder.decodeFrameAt(decoder.getDurationSeconds() * 0.5);
        if (!frame.isValid())
        {
            std::cerr << "Frame decode failed: " << decoder.getLastError() << '\n';
            return 32;
        }
        std::cout << "Decoded " << frame.getWidth() << 'x' << frame.getHeight()
                  << " frame from " << decoder.getDurationSeconds() << " second MP4\n";
        return 0;
    }

    if (argc == 2 && std::string_view(argv[1]) == "--playback-logic-only")
        return runPlaybackLogicSelfTest() ? 0 : 29;

    if (argc == 2 && std::string_view(argv[1]) == "--response-routing-only")
    {
        using dawvr::detail::responseCompletesRequest;
        using dawvr::protocol::ResponseKind;
        if (responseCompletesRequest(ResponseKind::transportAccepted,
                                     ResponseKind::sessionStateChanged)
            || responseCompletesRequest(ResponseKind::transportAccepted,
                                        ResponseKind::recordingFinalized)
            || !responseCompletesRequest(ResponseKind::transportAccepted,
                                         ResponseKind::transportAccepted)
            || !responseCompletesRequest(ResponseKind::transportAccepted, ResponseKind::error))
        {
            std::cerr << "Pending-response routing self-test failed\n";
            return 28;
        }
        std::cout << "Pending-response routing self-test passed\n";
        return 0;
    }

    const auto testProjectRoot = juce::File::getSpecialLocation(juce::File::tempDirectory)
                                     .getNonexistentChildFile("dawvr-ipc-smoke", {}, true);
    if (testProjectRoot.createDirectory().failed())
    {
        std::cerr << "Could not create temporary project root\n";
        return 5;
    }

    dawvr::SpscQueue<dawvr::TransportSnapshot, 2048U> queue;
    dawvr::SpscQueue<dawvr::TransportSnapshot, 2048U> sharedQueue;
    std::atomic<bool> armed{};
    std::atomic<bool> sharedArmed{};
    dawvr::ServiceClient client(queue, armed);
    dawvr::ServiceClient sharedClient(sharedQueue, sharedArmed);
    client.setIdentity("ipc-smoke-test", "IPC Smoke Test");
    sharedClient.setIdentity("ipc-smoke-shared-test", "IPC Shared Camera Test");
    client.setProjectRoot(testProjectRoot.getFullPathName());
    sharedClient.setProjectRoot(testProjectRoot.getFullPathName());
    client.setEncodingProfile(8'000'000U, 2'000U, true);
    sharedClient.setEncodingProfile(8'000'000U, 2'000U, true);
    client.setSyncOffsetNs(50'000'000);
    sharedClient.setSyncOffsetNs(50'000'000);
    const auto stopClients = [&client, &sharedClient] {
        client.stop();
        sharedClient.stop();
    };
    client.start();

    const auto connectionDeadline = juce::Time::getMillisecondCounter() + 5000U;
    while (client.connectionState() != dawvr::ServiceConnectionState::connected
           && juce::Time::getMillisecondCounter() < connectionDeadline)
        juce::Thread::sleep(20);

    if (client.connectionState() != dawvr::ServiceConnectionState::connected)
    {
        std::cerr << "Connection failed: " << client.statusText() << '\n';
        stopClients();
        return 1;
    }

    const auto sourceDeadline = juce::Time::getMillisecondCounter() + 5000U;
    while (client.sourceListRevision() == 0U
           && juce::Time::getMillisecondCounter() < sourceDeadline)
        juce::Thread::sleep(20);

    if (client.sourceListRevision() == 0U)
    {
        std::cerr << "Camera source list was not received\n";
        stopClients();
        return 4;
    }

    const auto sources = client.availableSources();
    const auto exerciseCapture = !sources.empty();
    if (exerciseCapture)
    {
        sharedClient.start();
        const auto sharedConnectionDeadline = juce::Time::getMillisecondCounter() + 5000U;
        while (sharedClient.connectionState() != dawvr::ServiceConnectionState::connected
               && juce::Time::getMillisecondCounter() < sharedConnectionDeadline)
            juce::Thread::sleep(20);
        if (sharedClient.connectionState() != dawvr::ServiceConnectionState::connected)
        {
            std::cerr << "Shared client connection failed: " << sharedClient.statusText() << '\n';
            stopClients();
            return 18;
        }
        const auto sharedSourceDeadline = juce::Time::getMillisecondCounter() + 5000U;
        while (sharedClient.sourceListRevision() == 0U
               && juce::Time::getMillisecondCounter() < sharedSourceDeadline)
            juce::Thread::sleep(20);
        if (sharedClient.sourceListRevision() == 0U)
        {
            std::cerr << "Shared client camera source list was not received\n";
            stopClients();
            return 19;
        }

        const auto selectedSource = juce::String::fromUTF8(sources.front().sourceId.c_str());
        client.setSelectedSource(selectedSource);
        sharedClient.setSelectedSource(selectedSource);
        armed.store(true, std::memory_order_release);
        sharedArmed.store(true, std::memory_order_release);
        // Let both clients prepare and populate the encoded GOP buffer before preview traffic
        // begins sharing the control pipe. This mirrors the GUI flow where source/project
        // configuration is already settled before the user creates a DAW record edge.
        juce::Thread::sleep(5000);
        client.setPreviewEnabled(true);
        sharedClient.setPreviewEnabled(true);
        juce::Thread::sleep(1000);
    }

    dawvr::TransportSnapshot startSnapshot;
    startSnapshot.sequenceNumber = 1U;
    startSnapshot.callbackMonotonicTimeNs = monotonicTimeNs();
    startSnapshot.projectSamplePosition = 512;
    startSnapshot.sampleRate = 48000.0;
    startSnapshot.tempo = 120.0;
    startSnapshot.flags = dawvr::TransportFlags::playing
                          | dawvr::TransportFlags::projectSampleValid
                          | dawvr::TransportFlags::tempoValid;
    if (exerciseCapture)
        startSnapshot.flags |= dawvr::TransportFlags::recording;
    if (!queue.push(startSnapshot))
    {
        std::cerr << "Queue unexpectedly full\n";
        stopClients();
        return 2;
    }
    if (exerciseCapture && !sharedQueue.push(startSnapshot))
    {
        std::cerr << "Shared client queue unexpectedly full\n";
        stopClients();
        return 20;
    }

    const auto sendDeadline = juce::Time::getMillisecondCounter() + 5000U;
    while ((client.sentSnapshotCount() == 0U
            || (exerciseCapture && sharedClient.sentSnapshotCount() == 0U))
           && juce::Time::getMillisecondCounter() < sendDeadline)
        juce::Thread::sleep(20);
    if (client.sentSnapshotCount() == 0U
        || (exerciseCapture && sharedClient.sentSnapshotCount() == 0U))
    {
        std::cerr << "Concurrent recording starts were not acknowledged: "
                  << client.statusText() << " / " << sharedClient.statusText() << '\n';
        stopClients();
        return 21;
    }
    if (client.connectionState() != dawvr::ServiceConnectionState::connected
        || (exerciseCapture
            && sharedClient.connectionState() != dawvr::ServiceConnectionState::connected))
    {
        std::cerr << "Recording start caused an IPC reconnect: " << client.statusText()
                  << " / " << sharedClient.statusText() << '\n';
        stopClients();
        return 26;
    }

    dawvr::FinalizedTakeInfo firstFinalizedTake;
    dawvr::FinalizedTakeInfo sharedFinalizedTake;
    std::uint32_t preparedStartAcknowledgementMs{};
    if (exerciseCapture)
    {
        juce::Thread::sleep(1500);
        auto stopSnapshot = startSnapshot;
        stopSnapshot.sequenceNumber = 2U;
        stopSnapshot.callbackMonotonicTimeNs = monotonicTimeNs();
        stopSnapshot.projectSamplePosition = 72512;
        stopSnapshot.flags &= ~dawvr::TransportFlags::recording;
        if (!queue.push(stopSnapshot))
        {
            std::cerr << "Queue unexpectedly full while stopping\n";
            stopClients();
            return 6;
        }
        if (!sharedQueue.push(stopSnapshot))
        {
            std::cerr << "Shared client queue unexpectedly full while stopping\n";
            stopClients();
            return 22;
        }

        const auto finalizationDeadline = juce::Time::getMillisecondCounter() + 15000U;
        while ((client.latestFinalizedTake().revision == 0U
                || sharedClient.latestFinalizedTake().revision == 0U)
               && juce::Time::getMillisecondCounter() < finalizationDeadline)
            juce::Thread::sleep(20);
        if (client.latestFinalizedTake().revision == 0U
            || sharedClient.latestFinalizedTake().revision == 0U)
        {
            std::cerr << "Concurrent recordings were not finalized: " << client.statusText()
                      << " / " << sharedClient.statusText() << '\n';
            stopClients();
            return 7;
        }
        if (client.connectionState() != dawvr::ServiceConnectionState::connected
            || sharedClient.connectionState() != dawvr::ServiceConnectionState::connected)
        {
            std::cerr << "Recording finalization caused an IPC reconnect: "
                      << client.statusText() << " / " << sharedClient.statusText() << '\n';
            stopClients();
            return 27;
        }
        firstFinalizedTake = client.latestFinalizedTake();
        sharedFinalizedTake = sharedClient.latestFinalizedTake();

        const auto firstStopAcknowledgementDeadline =
            juce::Time::getMillisecondCounter() + 10000U;
        while ((client.sentSnapshotCount() < 2U || sharedClient.sentSnapshotCount() < 2U)
               && juce::Time::getMillisecondCounter() < firstStopAcknowledgementDeadline)
            juce::Thread::sleep(20);
        if (client.sentSnapshotCount() < 2U || sharedClient.sentSnapshotCount() < 2U)
        {
            std::cerr << "Concurrent recording stops were not acknowledged: "
                      << client.statusText() << " / " << sharedClient.statusText() << '\n';
            stopClients();
            return 16;
        }

        auto secondStartSnapshot = startSnapshot;
        secondStartSnapshot.sequenceNumber = 3U;
        secondStartSnapshot.callbackMonotonicTimeNs = monotonicTimeNs();
        secondStartSnapshot.projectSamplePosition = 96512;
        const auto secondStartQueuedAt = juce::Time::getMillisecondCounter();
        if (!queue.push(secondStartSnapshot))
        {
            std::cerr << "Queue unexpectedly full during second start\n";
            stopClients();
            return 10;
        }

        const auto secondStartDeadline = juce::Time::getMillisecondCounter() + 10000U;
        while (client.sentSnapshotCount() < 3U
               && juce::Time::getMillisecondCounter() < secondStartDeadline)
            juce::Thread::sleep(20);
        if (client.sentSnapshotCount() < 3U)
        {
            std::cerr << "Prepared capture did not start a second take: " << client.statusText()
                      << '\n';
            stopClients();
            return 11;
        }
        preparedStartAcknowledgementMs =
            juce::Time::getMillisecondCounter() - secondStartQueuedAt;

        juce::Thread::sleep(1000);
        auto secondStopSnapshot = secondStartSnapshot;
        secondStopSnapshot.sequenceNumber = 4U;
        secondStopSnapshot.callbackMonotonicTimeNs = monotonicTimeNs();
        secondStopSnapshot.projectSamplePosition = 144512;
        secondStopSnapshot.flags &= ~dawvr::TransportFlags::recording;
        if (!queue.push(secondStopSnapshot))
        {
            std::cerr << "Queue unexpectedly full during second stop\n";
            stopClients();
            return 12;
        }

        const auto secondFinalizationDeadline = juce::Time::getMillisecondCounter() + 15000U;
        while (client.latestFinalizedTake().revision <= firstFinalizedTake.revision
               && juce::Time::getMillisecondCounter() < secondFinalizationDeadline)
            juce::Thread::sleep(20);
        if (client.latestFinalizedTake().revision <= firstFinalizedTake.revision)
        {
            std::cerr << "Second recording was not finalized: " << client.statusText() << '\n';
            stopClients();
            return 13;
        }

        const auto secondStopAcknowledgementDeadline =
            juce::Time::getMillisecondCounter() + 10000U;
        while (client.sentSnapshotCount() < 4U
               && juce::Time::getMillisecondCounter() < secondStopAcknowledgementDeadline)
            juce::Thread::sleep(20);
        if (client.sentSnapshotCount() < 4U)
        {
            std::cerr << "Second recording stop was not acknowledged: "
                      << client.statusText() << '\n';
            stopClients();
            return 25;
        }
    }

    const auto sent = client.sentSnapshotCount();
    const auto sharedSent = sharedClient.sentSnapshotCount();
    const auto finalizedTake = client.latestFinalizedTake();
    const auto status = client.statusText();
    const auto sharedStatus = sharedClient.statusText();
    stopClients();

    const auto expectedSnapshots = exerciseCapture ? 4U : 1U;
    if (sent != expectedSnapshots)
    {
        std::cerr << "Transport acknowledgement was not received: " << status << '\n';
        return 3;
    }
    if (exerciseCapture && sharedSent != 2U)
    {
        std::cerr << "Shared transport acknowledgement was not received: " << sharedStatus
                  << '\n';
        return 23;
    }

    std::cout << "IPC smoke test passed; cameras detected: " << sources.size() << '\n';
    for (const auto& source : sources)
        std::cout << "- " << source.displayName << '\n';
    if (exerciseCapture)
    {
        std::uint64_t totalBufferedFrames{};
        std::uint64_t totalDiscardedTailFrames{};
        const auto validateTake = [&totalBufferedFrames, &totalDiscardedTailFrames](
                                      const dawvr::FinalizedTakeInfo& take, const char* label) {
            const juce::File video(take.videoPath);
            const juce::File metadata(take.metadataPath);
            if (!video.existsAsFile() || video.getSize() == 0 || !metadata.existsAsFile())
            {
                std::cerr << label << " take artifacts are missing\n";
                return false;
            }
            const auto metadataJson = juce::JSON::parse(metadata.loadFileAsString());
            const auto* metadataObject = metadataJson.getDynamicObject();
            if (metadataObject == nullptr
                || static_cast<int>(metadataObject->getProperty("schema_version")) != 2
                || !metadataObject->hasProperty("first_frame_monotonic_time_ns")
                || !metadataObject->hasProperty("sync_offset_ns")
                || !metadataObject->hasProperty("buffered_frame_count")
                || !metadataObject->hasProperty("last_frame_monotonic_time_ns")
                || !metadataObject->hasProperty("end_sync_offset_ns")
                || !metadataObject->hasProperty("discarded_tail_frame_count")
                || !metadataObject->hasProperty("stop_holdback_exceeded")
                || !metadataObject->hasProperty(
                    "clock_drift_start_frame_monotonic_time_ns")
                || !metadataObject->hasProperty(
                    "clock_drift_end_frame_monotonic_time_ns"))
            {
                std::cerr << label << " take metadata has no timing correlation\n";
                return false;
            }
            const auto firstFrame =
                metadataObject->getProperty("first_frame_monotonic_time_ns");
            const auto syncOffset = static_cast<juce::int64>(
                metadataObject->getProperty("sync_offset_ns"));
            const auto bufferedFrames = static_cast<juce::int64>(
                metadataObject->getProperty("buffered_frame_count"));
            const auto lastFrame =
                metadataObject->getProperty("last_frame_monotonic_time_ns");
            const auto endSyncOffset = static_cast<juce::int64>(
                metadataObject->getProperty("end_sync_offset_ns"));
            const auto discardedTailFrames = static_cast<juce::int64>(
                metadataObject->getProperty("discarded_tail_frame_count"));
            const auto holdbackExceeded = static_cast<bool>(
                metadataObject->getProperty("stop_holdback_exceeded"));
            const auto configuredSyncOffset = static_cast<juce::int64>(
                metadataObject->getProperty("configured_sync_offset_ns"));
            const auto encodingBackend =
                metadataObject->getProperty("encoding_backend").toString();
            const auto driftStart = metadataObject->getProperty(
                "clock_drift_start_frame_monotonic_time_ns");
            const auto driftEnd = metadataObject->getProperty(
                "clock_drift_end_frame_monotonic_time_ns");
            if (firstFrame.isVoid() || syncOffset < -2500000000LL || syncOffset > 1000000000
                || bufferedFrames < 0 || lastFrame.isVoid() || endSyncOffset > 0
                || endSyncOffset < -1000000000 || discardedTailFrames < 0
                || holdbackExceeded || configuredSyncOffset != 50000000
                || encodingBackend.isEmpty()
                || !metadataObject->hasProperty("clock_drift_ns")
                || !metadataObject->hasProperty("clock_drift_ppm") || driftStart.isVoid()
                || driftEnd.isVoid())
            {
                std::cerr << label << " take timing correlation is invalid\n";
                return false;
            }
            totalBufferedFrames += static_cast<std::uint64_t>(bufferedFrames);
            totalDiscardedTailFrames += static_cast<std::uint64_t>(discardedTailFrames);
            std::cout << label << " MP4 bytes: " << video.getSize() << '\n';
            std::cout << label << " metadata: " << metadata.getFullPathName() << '\n';
            std::cout << label << " sync offset ns: " << syncOffset
                      << ", buffered frames: " << bufferedFrames << '\n';
            std::cout << label << " end sync offset ns: " << endSyncOffset
                      << ", discarded tail frames: " << discardedTailFrames << '\n';
            return true;
        };
        if (!validateTake(firstFinalizedTake, "Primary first")
            || !validateTake(sharedFinalizedTake, "Shared concurrent")
            || !validateTake(finalizedTake, "Primary second"))
        {
            return 8;
        }

        const auto validateRecordedPlayback = [](const dawvr::FinalizedTakeInfo& take) {
            const auto metadataJson =
                juce::JSON::parse(juce::File(take.metadataPath).loadFileAsString());
            const auto* metadataObject = metadataJson.getDynamicObject();
            if (metadataObject == nullptr)
                return false;

            dawvr::MFVideoDecoder decoder;
            if (!decoder.open(juce::File(take.videoPath)))
            {
                std::cerr << "Recorded decoder open failed: " << decoder.getLastError() << '\n';
                return false;
            }
            if (decoder.getDurationSeconds() <= 0.0)
            {
                std::cerr << "Recorded decoder reported no duration\n";
                return false;
            }

            const auto startSample = static_cast<juce::int64>(
                metadataObject->getProperty("start_project_sample"));
            const auto sampleRate = static_cast<double>(
                metadataObject->getProperty("sample_rate"));
            const auto syncOffset = static_cast<juce::int64>(
                metadataObject->getProperty("sync_offset_ns"));
            const auto middleVideoTime = decoder.getDurationSeconds() * 0.5;
            const auto middleProjectSample = startSample + static_cast<juce::int64>(
                std::llround((middleVideoTime
                              + static_cast<double>(syncOffset) / 1'000'000'000.0)
                             * sampleRate));
            const auto mappedTime = dawvr::mapProjectSampleToVideoTimeSeconds(
                middleProjectSample, startSample, sampleRate, syncOffset);
            if (!mappedTime.has_value()
                || std::abs(*mappedTime - middleVideoTime) > (1.0 / sampleRate))
            {
                return false;
            }

            const auto frame = decoder.decodeFrameAt(*mappedTime);
            if (!frame.isValid())
                std::cerr << "Recorded frame decode failed: " << decoder.getLastError() << '\n';
            return frame.isValid();
        };
        if (!validateRecordedPlayback(finalizedTake))
        {
            std::cerr << "Finalized MP4 could not be mapped and decoded for GUI playback\n";
            return 30;
        }
        std::cout << "Finalized MP4 GUI playback decode passed\n";
        if (firstFinalizedTake.videoPath == finalizedTake.videoPath
            || firstFinalizedTake.videoPath == sharedFinalizedTake.videoPath
            || finalizedTake.videoPath == sharedFinalizedTake.videoPath)
        {
            std::cerr << "Independent recordings reused an output path\n";
            return 14;
        }
        const auto timingValue = [](const dawvr::FinalizedTakeInfo& take,
                                    const char* property) {
            const auto metadataJson =
                juce::JSON::parse(juce::File(take.metadataPath).loadFileAsString());
            return static_cast<juce::int64>(
                metadataJson.getDynamicObject()->getProperty(property));
        };
        if (timingValue(firstFinalizedTake, "first_frame_monotonic_time_ns")
                != timingValue(sharedFinalizedTake, "first_frame_monotonic_time_ns")
            || timingValue(firstFinalizedTake, "last_frame_monotonic_time_ns")
                   != timingValue(sharedFinalizedTake, "last_frame_monotonic_time_ns"))
        {
            std::cerr << "Concurrent recordings did not receive the same shared frame range\n";
            return 24;
        }
        if (totalBufferedFrames == 0U)
        {
            std::cerr << "Prepared capture did not recover any frame from the ring buffer\n";
            return 15;
        }
        if (totalDiscardedTailFrames == 0U)
        {
            std::cerr << "Logical stop did not discard any IPC-latency tail frame\n";
            return 17;
        }
        std::cout << "Prepared start acknowledgement ms: "
                  << preparedStartAcknowledgementMs << '\n';
    }
    if (!testProjectRoot.deleteRecursively())
    {
        std::cerr << "Could not remove temporary project root\n";
        return 9;
    }
    return 0;
}
