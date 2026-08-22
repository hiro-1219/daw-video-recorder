// SPDX-License-Identifier: AGPL-3.0-only
#include "ServiceClient.h"

#include <juce_core/juce_core.h>

#include <atomic>
#include <iostream>

int main()
{
    const auto testProjectRoot = juce::File::getSpecialLocation(juce::File::tempDirectory)
                                     .getNonexistentChildFile("dawvr-ipc-smoke", {}, true);
    if (testProjectRoot.createDirectory().failed())
    {
        std::cerr << "Could not create temporary project root\n";
        return 5;
    }

    dawvr::SpscQueue<dawvr::TransportSnapshot, 2048U> queue;
    std::atomic<bool> armed{};
    dawvr::ServiceClient client(queue, armed);
    client.setIdentity("ipc-smoke-test", "IPC Smoke Test");
    client.setProjectRoot(testProjectRoot.getFullPathName());
    client.start();

    const auto connectionDeadline = juce::Time::getMillisecondCounter() + 5000U;
    while (client.connectionState() != dawvr::ServiceConnectionState::connected
           && juce::Time::getMillisecondCounter() < connectionDeadline)
        juce::Thread::sleep(20);

    if (client.connectionState() != dawvr::ServiceConnectionState::connected)
    {
        std::cerr << "Connection failed: " << client.statusText() << '\n';
        client.stop();
        return 1;
    }

    const auto sourceDeadline = juce::Time::getMillisecondCounter() + 5000U;
    while (client.sourceListRevision() == 0U
           && juce::Time::getMillisecondCounter() < sourceDeadline)
        juce::Thread::sleep(20);

    if (client.sourceListRevision() == 0U)
    {
        std::cerr << "Camera source list was not received\n";
        client.stop();
        return 4;
    }

    const auto sources = client.availableSources();
    const auto exerciseCapture = !sources.empty();
    if (exerciseCapture)
    {
        client.setSelectedSource(juce::String::fromUTF8(sources.front().sourceId.c_str()));
        armed.store(true, std::memory_order_release);
        juce::Thread::sleep(500);
    }

    dawvr::TransportSnapshot startSnapshot;
    startSnapshot.sequenceNumber = 1U;
    startSnapshot.callbackMonotonicTimeNs = 1000;
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
        client.stop();
        return 2;
    }

    const auto sendDeadline = juce::Time::getMillisecondCounter() + 5000U;
    while (client.sentSnapshotCount() == 0U
           && juce::Time::getMillisecondCounter() < sendDeadline)
        juce::Thread::sleep(20);

    dawvr::FinalizedTakeInfo firstFinalizedTake;
    std::uint32_t preparedStartAcknowledgementMs{};
    if (exerciseCapture)
    {
        juce::Thread::sleep(1500);
        auto stopSnapshot = startSnapshot;
        stopSnapshot.sequenceNumber = 2U;
        stopSnapshot.callbackMonotonicTimeNs = 1500001000;
        stopSnapshot.projectSamplePosition = 72512;
        stopSnapshot.flags &= ~dawvr::TransportFlags::recording;
        if (!queue.push(stopSnapshot))
        {
            std::cerr << "Queue unexpectedly full while stopping\n";
            client.stop();
            return 6;
        }

        const auto finalizationDeadline = juce::Time::getMillisecondCounter() + 15000U;
        while (client.latestFinalizedTake().revision == 0U
               && juce::Time::getMillisecondCounter() < finalizationDeadline)
            juce::Thread::sleep(20);
        if (client.latestFinalizedTake().revision == 0U)
        {
            std::cerr << "Recording was not finalized: " << client.statusText() << '\n';
            client.stop();
            return 7;
        }
        firstFinalizedTake = client.latestFinalizedTake();

        auto secondStartSnapshot = startSnapshot;
        secondStartSnapshot.sequenceNumber = 3U;
        secondStartSnapshot.callbackMonotonicTimeNs = 2000001000;
        secondStartSnapshot.projectSamplePosition = 96512;
        const auto secondStartQueuedAt = juce::Time::getMillisecondCounter();
        if (!queue.push(secondStartSnapshot))
        {
            std::cerr << "Queue unexpectedly full during second start\n";
            client.stop();
            return 10;
        }

        const auto secondStartDeadline = juce::Time::getMillisecondCounter() + 5000U;
        while (client.sentSnapshotCount() < 3U
               && juce::Time::getMillisecondCounter() < secondStartDeadline)
            juce::Thread::sleep(20);
        if (client.sentSnapshotCount() < 3U)
        {
            std::cerr << "Prepared capture did not start a second take: " << client.statusText()
                      << '\n';
            client.stop();
            return 11;
        }
        preparedStartAcknowledgementMs =
            juce::Time::getMillisecondCounter() - secondStartQueuedAt;

        juce::Thread::sleep(1000);
        auto secondStopSnapshot = secondStartSnapshot;
        secondStopSnapshot.sequenceNumber = 4U;
        secondStopSnapshot.callbackMonotonicTimeNs = 3000001000;
        secondStopSnapshot.projectSamplePosition = 144512;
        secondStopSnapshot.flags &= ~dawvr::TransportFlags::recording;
        if (!queue.push(secondStopSnapshot))
        {
            std::cerr << "Queue unexpectedly full during second stop\n";
            client.stop();
            return 12;
        }

        const auto secondFinalizationDeadline = juce::Time::getMillisecondCounter() + 15000U;
        while (client.latestFinalizedTake().revision <= firstFinalizedTake.revision
               && juce::Time::getMillisecondCounter() < secondFinalizationDeadline)
            juce::Thread::sleep(20);
        if (client.latestFinalizedTake().revision <= firstFinalizedTake.revision)
        {
            std::cerr << "Second recording was not finalized: " << client.statusText() << '\n';
            client.stop();
            return 13;
        }
    }

    const auto sent = client.sentSnapshotCount();
    const auto finalizedTake = client.latestFinalizedTake();
    const auto status = client.statusText();
    client.stop();

    const auto expectedSnapshots = exerciseCapture ? 4U : 1U;
    if (sent != expectedSnapshots)
    {
        std::cerr << "Transport acknowledgement was not received: " << status << '\n';
        return 3;
    }

    std::cout << "IPC smoke test passed; cameras detected: " << sources.size() << '\n';
    for (const auto& source : sources)
        std::cout << "- " << source.displayName << '\n';
    if (exerciseCapture)
    {
        const auto validateTake = [](const dawvr::FinalizedTakeInfo& take, const char* label) {
            const juce::File video(take.videoPath);
            const juce::File metadata(take.metadataPath);
            if (!video.existsAsFile() || video.getSize() == 0 || !metadata.existsAsFile())
            {
                std::cerr << label << " take artifacts are missing\n";
                return false;
            }
            std::cout << label << " MP4 bytes: " << video.getSize() << '\n';
            std::cout << label << " metadata: " << metadata.getFullPathName() << '\n';
            return true;
        };
        if (!validateTake(firstFinalizedTake, "First")
            || !validateTake(finalizedTake, "Second"))
        {
            return 8;
        }
        if (firstFinalizedTake.videoPath == finalizedTake.videoPath)
        {
            std::cerr << "Repeated recording reused the previous output path\n";
            return 14;
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
