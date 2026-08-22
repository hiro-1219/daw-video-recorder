// SPDX-License-Identifier: AGPL-3.0-only
#include "ServiceClient.h"

#include <array>
#include <vector>

namespace dawvr
{
namespace
{
constexpr auto pipeName = "daw-video-recorder-v1";
constexpr auto maximumMessageBytes = 1024U * 1024U;
constexpr auto ioTimeoutMilliseconds = 1500;
constexpr auto maximumBatchSize = 64U;
} // namespace

ServiceClient::ServiceClient(SpscQueue<TransportSnapshot, 2048U>& queueToDrain,
                             std::atomic<bool>& recordArmState)
    : Thread("DAW Video Recorder IPC"), queue(queueToDrain), recordArm(recordArmState)
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
    pipe.close();
    notify();
    stopThread(2000);
    setStatus(ServiceConnectionState::stopped, "Stopped");
}

void ServiceClient::setIdentity(juce::String persistentInstanceId, juce::String instanceLabel)
{
    const juce::ScopedLock lock(configurationLock);
    if (persistentId == persistentInstanceId && label == instanceLabel)
        return;
    persistentId = std::move(persistentInstanceId);
    label = std::move(instanceLabel);
    configurationRevision.fetch_add(1U, std::memory_order_release);
    pipe.close();
    notify();
}

void ServiceClient::setProjectRoot(juce::String projectRoot)
{
    const juce::ScopedLock lock(configurationLock);
    if (configuredProjectRoot == projectRoot)
        return;
    configuredProjectRoot = std::move(projectRoot);
    configurationRevision.fetch_add(1U, std::memory_order_release);
    notify();
}

bool ServiceClient::isWorkerRunning() const noexcept
{
    return isThreadRunning();
}

ServiceConnectionState ServiceClient::connectionState() const noexcept
{
    return currentState.load(std::memory_order_acquire);
}

juce::String ServiceClient::statusText() const
{
    const juce::ScopedLock lock(statusLock);
    return currentStatus;
}

std::uint64_t ServiceClient::consumedSnapshotCount() const noexcept
{
    return consumedCount.load(std::memory_order_relaxed);
}

std::uint64_t ServiceClient::sentSnapshotCount() const noexcept
{
    return transmittedCount.load(std::memory_order_relaxed);
}

void ServiceClient::run()
{
    while (!threadShouldExit())
    {
        if (!pipe.isOpen() && !connectAndRegister())
        {
            TransportSnapshot discarded;
            while (queue.pop(discarded))
                consumedCount.fetch_add(1U, std::memory_order_relaxed);
            wait(500);
            continue;
        }

        if (!synchronizeConfiguration() || !sendTransportBatch())
        {
            disconnect();
            wait(250);
            continue;
        }

        wait(5);
    }

    disconnect();
}

bool ServiceClient::connectAndRegister()
{
    juce::String identity;
    juce::String instanceLabel;
    {
        const juce::ScopedLock lock(configurationLock);
        identity = persistentId;
        instanceLabel = label;
    }

    if (identity.isEmpty())
    {
        setStatus(ServiceConnectionState::connecting, "Waiting for plug-in identity");
        return false;
    }

    setStatus(ServiceConnectionState::connecting, "Connecting to recording service...");
    if (!pipe.openExisting(pipeName))
    {
        setStatus(ServiceConnectionState::error, "Recording service is not running");
        return false;
    }

    protocol::Response response;
    auto requestId = nextRequestId();
    if (!exchange(protocol::makeHello(requestId),
                  requestId,
                  protocol::ResponseKind::helloAck,
                  response)
        || response.selectedVersion != 1U)
    {
        setStatus(ServiceConnectionState::error, "Protocol negotiation failed");
        pipe.close();
        return false;
    }
    const auto serviceVersion = response.serviceVersion;

    requestId = nextRequestId();
    if (!exchange(protocol::makeRegisterInstance(requestId,
                                                 identity.toStdString(),
                                                 instanceLabel.toStdString()),
                  requestId,
                  protocol::ResponseKind::instanceRegistered,
                  response)
        || response.runtimeInstanceId.empty())
    {
        setStatus(ServiceConnectionState::error, "Instance registration failed");
        pipe.close();
        return false;
    }

    runtimeInstanceId = response.runtimeInstanceId;
    appliedConfigurationRevision = 0U;
    lastSentProjectRoot.clear();
    lastSentArmed = false;
    setStatus(ServiceConnectionState::connected,
              "Connected to recording service " + juce::String(serviceVersion));
    return true;
}

bool ServiceClient::synchronizeConfiguration()
{
    const auto revision = configurationRevision.load(std::memory_order_acquire);
    const auto armed = recordArm.load(std::memory_order_acquire);
    juce::String projectRoot;
    {
        const juce::ScopedLock lock(configurationLock);
        projectRoot = configuredProjectRoot;
    }
    const auto root = projectRoot.toStdString();

    protocol::Response response;
    if (!root.empty() && root != lastSentProjectRoot)
    {
        const auto requestId = nextRequestId();
        if (!exchange(protocol::makeSetProjectRoot(requestId, runtimeInstanceId, root),
                      requestId,
                      protocol::ResponseKind::sessionStateChanged,
                      response))
            return false;
        lastSentProjectRoot = root;
    }

    if (armed != lastSentArmed || revision != appliedConfigurationRevision)
    {
        const auto requestId = nextRequestId();
        if (!exchange(protocol::makeSetArmed(requestId, runtimeInstanceId, armed),
                      requestId,
                      protocol::ResponseKind::sessionStateChanged,
                      response))
            return false;
        lastSentArmed = armed;
    }

    appliedConfigurationRevision = revision;
    return true;
}

bool ServiceClient::sendTransportBatch()
{
    std::vector<TransportSnapshot> snapshots;
    snapshots.reserve(maximumBatchSize);
    TransportSnapshot snapshot;
    while (snapshots.size() < maximumBatchSize && queue.pop(snapshot))
    {
        snapshots.push_back(snapshot);
        consumedCount.fetch_add(1U, std::memory_order_relaxed);
    }

    if (snapshots.empty())
        return true;

    const auto requestId = nextRequestId();
    protocol::Response response;
    if (!exchange(protocol::makeTransportBatch(requestId, runtimeInstanceId, snapshots),
                  requestId,
                  protocol::ResponseKind::transportAccepted,
                  response))
        return false;

    transmittedCount.fetch_add(snapshots.size(), std::memory_order_relaxed);
    return true;
}

bool ServiceClient::sendMessage(const protocol::Bytes& message)
{
    if (message.size() > maximumMessageBytes)
        return false;

    const auto length = static_cast<std::uint32_t>(message.size());
    const std::array<std::uint8_t, 4> header {
        static_cast<std::uint8_t>(length & 0xffU),
        static_cast<std::uint8_t>((length >> 8U) & 0xffU),
        static_cast<std::uint8_t>((length >> 16U) & 0xffU),
        static_cast<std::uint8_t>((length >> 24U) & 0xffU)
    };
    return pipe.write(header.data(), static_cast<int>(header.size()), ioTimeoutMilliseconds)
               == static_cast<int>(header.size())
           && pipe.write(message.data(), static_cast<int>(message.size()), ioTimeoutMilliseconds)
                  == static_cast<int>(message.size());
}

bool ServiceClient::receiveResponse(protocol::Response& response, int timeoutMilliseconds)
{
    std::array<std::uint8_t, 4> header{};
    if (pipe.read(header.data(), static_cast<int>(header.size()), timeoutMilliseconds)
        != static_cast<int>(header.size()))
        return false;

    const auto length = static_cast<std::uint32_t>(header[0])
                        | (static_cast<std::uint32_t>(header[1]) << 8U)
                        | (static_cast<std::uint32_t>(header[2]) << 16U)
                        | (static_cast<std::uint32_t>(header[3]) << 24U);
    if (length == 0U || length > maximumMessageBytes)
        return false;

    protocol::Bytes payload(length);
    if (pipe.read(payload.data(), static_cast<int>(payload.size()), timeoutMilliseconds)
        != static_cast<int>(payload.size()))
        return false;
    return protocol::decodeResponse(payload, response);
}

bool ServiceClient::exchange(const protocol::Bytes& message,
                             std::uint64_t requestId,
                             protocol::ResponseKind expectedKind,
                             protocol::Response& response)
{
    if (!sendMessage(message))
        return false;

    for (auto remainingResponses = 0; remainingResponses < 8; ++remainingResponses)
    {
        if (!receiveResponse(response, ioTimeoutMilliseconds))
            return false;
        if (response.kind == protocol::ResponseKind::error)
        {
            setStatus(ServiceConnectionState::error,
                      juce::String(response.errorCode) + ": " + juce::String(response.message));
            return false;
        }
        if (response.requestId == requestId && response.kind == expectedKind)
            return true;
    }
    return false;
}

void ServiceClient::disconnect()
{
    if (pipe.isOpen() && !runtimeInstanceId.empty())
        static_cast<void>(sendMessage(protocol::makeUnregisterInstance(nextRequestId(), runtimeInstanceId)));
    pipe.close();
    runtimeInstanceId.clear();
    if (!threadShouldExit())
        setStatus(ServiceConnectionState::connecting, "Disconnected; retrying...");
}

void ServiceClient::setStatus(ServiceConnectionState state, juce::String text)
{
    currentState.store(state, std::memory_order_release);
    const juce::ScopedLock lock(statusLock);
    currentStatus = std::move(text);
}

std::uint64_t ServiceClient::nextRequestId() noexcept
{
    return ++requestSequence;
}
} // namespace dawvr
