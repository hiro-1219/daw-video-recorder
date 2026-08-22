// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "ProtocolCodec.h"
#include "SpscQueue.h"
#include "TransportSnapshot.h"

#include <juce_core/juce_core.h>

#include <atomic>
#include <string>
#include <vector>

namespace dawvr
{
enum class ServiceConnectionState
{
    stopped,
    connecting,
    connected,
    error
};

struct FinalizedTakeInfo final
{
    juce::String takeId;
    juce::String videoPath;
    juce::String metadataPath;
    std::uint64_t revision{};
};

class ServiceClient final : private juce::Thread
{
public:
    ServiceClient(SpscQueue<TransportSnapshot, 2048U>& queueToDrain,
                  std::atomic<bool>& recordArmState);
    ~ServiceClient() override;

    void start();
    void stop();
    void setIdentity(juce::String persistentInstanceId, juce::String instanceLabel);
    void setProjectRoot(juce::String projectRoot);
    void setSelectedSource(juce::String sourceId);
    void requestSourceRefresh() noexcept;

    [[nodiscard]] bool isWorkerRunning() const noexcept;
    [[nodiscard]] ServiceConnectionState connectionState() const noexcept;
    [[nodiscard]] juce::String statusText() const;
    [[nodiscard]] std::uint64_t consumedSnapshotCount() const noexcept;
    [[nodiscard]] std::uint64_t sentSnapshotCount() const noexcept;
    [[nodiscard]] std::uint64_t sourceListRevision() const noexcept;
    [[nodiscard]] std::vector<protocol::CameraSource> availableSources() const;
    [[nodiscard]] FinalizedTakeInfo latestFinalizedTake() const;

private:
    void run() override;
    bool connectAndRegister();
    bool synchronizeConfiguration();
    bool refreshSources();
    bool sendTransportBatch();
    bool sendMessage(const protocol::Bytes& message);
    bool receiveResponse(protocol::Response& response, int timeoutMilliseconds);
    bool exchange(const protocol::Bytes& message,
                  std::uint64_t requestId,
                  protocol::ResponseKind expectedKind,
                  protocol::Response& response);
    void disconnect();
    void setStatus(ServiceConnectionState state, juce::String text);
    void storeFinalizedTake(const protocol::Response& response);
    [[nodiscard]] std::uint64_t nextRequestId() noexcept;

    SpscQueue<TransportSnapshot, 2048U>& queue;
    std::atomic<bool>& recordArm;
    juce::NamedPipe pipe;

    mutable juce::CriticalSection configurationLock;
    juce::String persistentId;
    juce::String label { "Camera" };
    juce::String configuredProjectRoot;
    juce::String configuredSourceId;
    std::atomic<std::uint64_t> configurationRevision { 1U };
    std::atomic<bool> sourceRefreshRequested { true };

    mutable juce::CriticalSection sourceLock;
    std::vector<protocol::CameraSource> sources;

    mutable juce::CriticalSection finalizedTakeLock;
    FinalizedTakeInfo finalizedTake;

    mutable juce::CriticalSection statusLock;
    juce::String currentStatus { "Stopped" };
    std::atomic<ServiceConnectionState> currentState { ServiceConnectionState::stopped };

    std::string runtimeInstanceId;
    std::uint64_t appliedConfigurationRevision{};
    bool lastSentArmed{};
    std::string lastSentProjectRoot;
    std::string lastSentSourceId;
    std::int64_t nextSourceRefreshTimeMs{};
    std::uint64_t requestSequence{};
    std::atomic<std::uint64_t> consumedCount{};
    std::atomic<std::uint64_t> transmittedCount{};
    std::atomic<std::uint64_t> receivedSourceListRevision{};
};
} // namespace dawvr
