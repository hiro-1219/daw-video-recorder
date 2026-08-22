// SPDX-License-Identifier: AGPL-3.0-only
#include "ServiceClient.h"

namespace dawvr
{
ServiceClient::ServiceClient(SpscQueue<TransportSnapshot, 2048U>& queueToDrain)
    : Thread("DAW Video Recorder IPC"), queue(queueToDrain)
{
}

ServiceClient::~ServiceClient()
{
    stop();
}

void ServiceClient::start()
{
    if (!isThreadRunning())
        startThread(juce::Thread::Priority::normal);
}

void ServiceClient::stop()
{
    signalThreadShouldExit();
    notify();
    stopThread(2000);
}

bool ServiceClient::isWorkerRunning() const noexcept
{
    return isThreadRunning();
}

std::uint64_t ServiceClient::consumedSnapshotCount() const noexcept
{
    return consumedCount.load(std::memory_order_relaxed);
}

TransportSnapshot ServiceClient::latestSnapshot() const noexcept
{
    const juce::ScopedLock lock(latestSnapshotLock);
    return lastSnapshot;
}

void ServiceClient::run()
{
    while (!threadShouldExit())
    {
        TransportSnapshot snapshot;
        bool consumedAny = false;

        while (queue.pop(snapshot))
        {
            consumedAny = true;
            consumedCount.fetch_add(1U, std::memory_order_relaxed);
            latestSequence.store(snapshot.sequenceNumber, std::memory_order_relaxed);

            const juce::ScopedLock lock(latestSnapshotLock);
            lastSnapshot = snapshot;
        }

        // Protocol serialization and Named Pipe writes are intentionally confined to this thread.
        // The first implementation milestone only drains and validates the real-time handoff.
        if (!consumedAny)
            wait(5);
    }
}
} // namespace dawvr

