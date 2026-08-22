// SPDX-License-Identifier: AGPL-3.0-only
#include "ServiceClient.h"

#include <juce_core/juce_core.h>

#include <atomic>
#include <iostream>

int main()
{
    dawvr::SpscQueue<dawvr::TransportSnapshot, 2048U> queue;
    std::atomic<bool> armed{};
    dawvr::ServiceClient client(queue, armed);
    client.setIdentity("ipc-smoke-test", "IPC Smoke Test");
    client.setProjectRoot(juce::File::getCurrentWorkingDirectory().getFullPathName());
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

    dawvr::TransportSnapshot snapshot;
    snapshot.sequenceNumber = 1U;
    snapshot.callbackMonotonicTimeNs = 1000;
    snapshot.projectSamplePosition = 512;
    snapshot.sampleRate = 48000.0;
    snapshot.tempo = 120.0;
    snapshot.flags = dawvr::TransportFlags::playing | dawvr::TransportFlags::projectSampleValid
                     | dawvr::TransportFlags::tempoValid;
    if (!queue.push(snapshot))
    {
        std::cerr << "Queue unexpectedly full\n";
        client.stop();
        return 2;
    }

    const auto sendDeadline = juce::Time::getMillisecondCounter() + 5000U;
    while (client.sentSnapshotCount() == 0U
           && juce::Time::getMillisecondCounter() < sendDeadline)
        juce::Thread::sleep(20);

    const auto sent = client.sentSnapshotCount();
    const auto sourceCount = client.availableSources().size();
    const auto status = client.statusText();
    client.stop();

    if (sent != 1U)
    {
        std::cerr << "Transport acknowledgement was not received: " << status << '\n';
        return 3;
    }

    std::cout << "IPC smoke test passed; cameras detected: " << sourceCount << '\n';
    return 0;
}
