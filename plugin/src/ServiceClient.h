// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "SpscQueue.h"
#include "TransportSnapshot.h"

#include <juce_core/juce_core.h>

#include <atomic>

namespace dawvr
{
class ServiceClient final : private juce::Thread
{
public:
    explicit ServiceClient(SpscQueue<TransportSnapshot, 2048U>& queueToDrain);
    ~ServiceClient() override;

    void start();
    void stop();

    [[nodiscard]] bool isWorkerRunning() const noexcept;
    [[nodiscard]] std::uint64_t consumedSnapshotCount() const noexcept;
    [[nodiscard]] TransportSnapshot latestSnapshot() const noexcept;

private:
    void run() override;

    SpscQueue<TransportSnapshot, 2048U>& queue;
    std::atomic<std::uint64_t> consumedCount{};
    std::atomic<std::uint64_t> latestSequence{};
    TransportSnapshot lastSnapshot{};
    juce::CriticalSection latestSnapshotLock;
};
} // namespace dawvr

