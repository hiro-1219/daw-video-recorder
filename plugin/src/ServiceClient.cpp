// SPDX-License-Identifier: AGPL-3.0-only
#include "ServiceClient.h"

#include <chrono>
#include <cstring>
#include <utility>

namespace dawvr
{
namespace
{
#if JUCE_WINDOWS
constexpr auto pipeName = R"(\\.\pipe\daw-video-recorder)";
#else
constexpr auto pipeName = "/tmp/daw-video-recorder.sock";
#endif

constexpr std::size_t maximumBatchSize = 64U;
constexpr std::size_t maximumMessageBytes = 4 * 1024 * 1024;
constexpr int ioTimeoutMilliseconds = 3'000;

bool tryLaunchRecordingService()
{
#if JUCE_WINDOWS
    const auto localAppDataStr = juce::Process::getEnvironmentVariableValue("LOCALAPPDATA");
    if (localAppDataStr.isNotEmpty())
    {
        const auto localExe = juce::File(localAppDataStr)
                                  .getChildFile("DAW Video Recorder")
                                  .getChildFile("daw-video-recorder-service.exe");
        if (localExe.existsAsFile())
            return localExe.startAsProcess();
    }

    const auto appData = juce::File::getSpecialLocation(
        juce::File::SpecialLocationType::userApplicationDataDirectory);
    const auto roamingExe = appData.getChildFile("DAW Video Recorder")
                               .getChildFile("daw-video-recorder-service.exe");
    if (roamingExe.existsAsFile())
        return roamingExe.startAsProcess();

    return false;
#else
    return false;
#endif
}
} // namespace

ServiceClient::ReceiverThread::ReceiverThread(ServiceClient& owner)
    : Thread("ServiceClientReceiver"), client(owner)
{
}

ServiceClient::ReceiverThread::~ReceiverThread()
{
    stopThread(2000);
}

void ServiceClient::ReceiverThread::run()
{
    while (!threadShouldExit())
    {
        if (!client.pipe.isOpen())
        {
            wait(50);
            continue;
        }

        protocol::Response response;
        if (!client.receiveResponse(response, 1000))
        {
            if (threadShouldExit() || !client.pipe.isOpen())
                break;
            continue;
        }

        if (response.kind == protocol::ResponseKind::previewFrame)
        {
            client.storePreviewFrame(response);
            continue;
        }

        if (response.kind == protocol::ResponseKind::recordingFinalized)
        {
            client.storeFinalizedTake(response);
        }

        if (response.requestId != 0U)
        {
            const juce::ScopedLock lock(client.pendingLock);
            auto it = client.pendingRequests.find(response.requestId);
            if (it != client.pendingRequests.end())
            {
                it->second->response = std::move(response);
                it->second->ready = true;
                it->second->event.signal();
            }
        }
    }
}

ServiceClient::ServiceClient(SpscQueue<TransportSnapshot, 2048U>& transportQueue,
                             std::atomic<bool>& recordArmState)
    : Thread("ServiceClientWorker"), queue(transportQueue), recordArm(recordArmState),
      receiver(*this)
{
}

ServiceClient::~ServiceClient()
{
    stop();
}

void ServiceClient::start()
{
    if (!isThreadRunning())
        startThread();
}

void ServiceClient::stop()
{
    signalThreadShouldExit();
    receiver.signalThreadShouldExit();
    notify();
    stopThread(2000);
    receiver.stopThread(2000);
    disconnect();
}

void ServiceClient::setIdentity(juce::String persistentInstanceId, juce::String instanceLabel)
{
    const juce::ScopedLock lock(configurationLock);
    persistentId = std::move(persistentInstanceId);
    label = std::move(instanceLabel);
    configurationRevision.fetch_add(1U, std::memory_order_release);
    notify();
}

void ServiceClient::setProjectRoot(juce::String projectRoot)
{
    const juce::ScopedLock lock(configurationLock);
    configuredProjectRoot = std::move(projectRoot);
    configurationRevision.fetch_add(1U, std::memory_order_release);
    notify();
}

void ServiceClient::setSelectedSource(juce::String sourceId)
{
    const juce::ScopedLock lock(configurationLock);
    configuredSourceId = std::move(sourceId);
    configurationRevision.fetch_add(1U, std::memory_order_release);
    notify();
}

void ServiceClient::setEncodingProfile(std::uint32_t bitRate,
                                      std::uint32_t gopDurationMs,
                                      bool preferHardware)
{
    const juce::ScopedLock lock(configurationLock);
    configuredBitRate = bitRate;
    configuredGopDurationMs = gopDurationMs;
    configuredPreferHardware = preferHardware;
    configurationRevision.fetch_add(1U, std::memory_order_release);
    notify();
}

void ServiceClient::setSyncOffsetNs(std::int64_t offsetNs)
{
    const juce::ScopedLock lock(configurationLock);
    configuredSyncOffsetNs = offsetNs;
    configurationRevision.fetch_add(1U, std::memory_order_release);
    notify();
}

void ServiceClient::setPreviewEnabled(bool enabled)
{
    previewEnabledState.store(enabled, std::memory_order_release);
    notify();
}

void ServiceClient::requestSourceRefresh() noexcept
{
    sourceRefreshRequested.store(true, std::memory_order_release);
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

std::uint64_t ServiceClient::sourceListRevision() const noexcept
{
    return receivedSourceListRevision.load(std::memory_order_acquire);
}

std::vector<protocol::CameraSource> ServiceClient::availableSources() const
{
    const juce::ScopedLock lock(sourceLock);
    return sources;
}

FinalizedTakeInfo ServiceClient::latestFinalizedTake() const
{
    const juce::ScopedLock lock(finalizedTakeLock);
    return finalizedTake;
}

ServiceClient::PreviewFrameData ServiceClient::latestPreviewFrame() const
{
    const juce::ScopedLock lock(previewLock);
    return currentPreviewFrame;
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

        const auto refreshDue = juce::Time::currentTimeMillis() >= nextSourceRefreshTimeMs;
        if ((refreshDue || sourceRefreshRequested.exchange(false, std::memory_order_acq_rel))
                && !refreshSources())
        {
            disconnect();
            wait(250);
            continue;
        }

        if (!synchronizeConfiguration() || !synchronizePreviewState() || !sendTransportBatch())
        {
            disconnect();
            wait(250);
            continue;
        }

        wait(10);
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
        const auto launched = tryLaunchRecordingService();
        setStatus(launched ? ServiceConnectionState::connecting : ServiceConnectionState::error,
                  launched ? "Recording service started; connecting..."
                           : "Recording service executable was not found");
        return false;
    }

    receiver.startThread();

    protocol::Response response;
    auto requestId = nextRequestId();
    if (!exchange(protocol::makeHello(requestId),
                  requestId,
                  protocol::ResponseKind::helloAck,
                  response)
        || response.selectedVersion != protocol::currentProtocolVersion)
    {
        setStatus(ServiceConnectionState::error, "Protocol negotiation failed");
        disconnect();
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
        setStatus(ServiceConnectionState::error, "Instance registration rejected");
        disconnect();
        return false;
    }

    runtimeInstanceId = response.runtimeInstanceId;
    lastSentProjectRoot.clear();
    lastSentSourceId.clear();
    lastSentBitRate = 0U;
    lastSentGopDurationMs = 0U;
    lastSentPreferHardware = false;
    lastSentSyncOffsetNs = std::numeric_limits<std::int64_t>::min();
    lastSentArmed = false;
    lastSentPreviewEnabled = false;
    appliedConfigurationRevision = 0U;
    nextSourceRefreshTimeMs = 0;

    setStatus(ServiceConnectionState::connected,
              "Connected to recording service " + juce::String::fromUTF8(serviceVersion.c_str()));
    return true;
}

bool ServiceClient::refreshSources()
{
    const auto requestId = nextRequestId();
    protocol::Response response;
    if (!exchange(protocol::makeListSources(requestId),
                  requestId,
                  protocol::ResponseKind::sourceList,
                  response))
        return false;

    {
        const juce::ScopedLock lock(sourceLock);
        sources = std::move(response.sources);
    }
    receivedSourceListRevision.fetch_add(1U, std::memory_order_release);
    nextSourceRefreshTimeMs = juce::Time::currentTimeMillis() + 5'000;
    return true;
}

bool ServiceClient::synchronizeConfiguration()
{
    const auto revision = configurationRevision.load(std::memory_order_acquire);
    const auto armed = recordArm.load(std::memory_order_acquire);

    juce::String projectRoot;
    juce::String sourceId;
    std::uint32_t bitRate{};
    std::uint32_t gopDurationMs{};
    bool preferHardware{};
    std::int64_t syncOffsetNs{};
    {
        const juce::ScopedLock lock(configurationLock);
        projectRoot = configuredProjectRoot;
        sourceId = configuredSourceId;
        bitRate = configuredBitRate;
        gopDurationMs = configuredGopDurationMs;
        preferHardware = configuredPreferHardware;
        syncOffsetNs = configuredSyncOffsetNs;
    }
    const auto root = projectRoot.toStdString();
    const auto source = sourceId.toStdString();

    protocol::Response response;
    if (!source.empty() && source != lastSentSourceId)
    {
        const auto requestId = nextRequestId();
        if (!exchange(protocol::makeSelectSource(requestId, runtimeInstanceId, source),
                      requestId,
                      protocol::ResponseKind::sessionStateChanged,
                      response))
            return false;
        lastSentSourceId = source;
        lastSentPreviewEnabled = false;
        if (previewEnabledState.load(std::memory_order_acquire))
        {
            const auto previewReqId = nextRequestId();
            sendMessage(protocol::makeRequestPreview(previewReqId, runtimeInstanceId, 480U, 270U, 30U));
            lastSentPreviewEnabled = true;
        }
    }

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

    if (bitRate != lastSentBitRate || gopDurationMs != lastSentGopDurationMs
        || preferHardware != lastSentPreferHardware)
    {
        const auto requestId = nextRequestId();
        if (!exchange(protocol::makeConfigureEncodingProfile(requestId,
                                                             runtimeInstanceId,
                                                             bitRate,
                                                             gopDurationMs,
                                                             preferHardware),
                      requestId,
                      protocol::ResponseKind::sessionStateChanged,
                      response))
            return false;
        lastSentBitRate = bitRate;
        lastSentGopDurationMs = gopDurationMs;
        lastSentPreferHardware = preferHardware;
    }

    if (syncOffsetNs != lastSentSyncOffsetNs)
    {
        const auto requestId = nextRequestId();
        if (!exchange(protocol::makeSetSyncOffset(requestId, runtimeInstanceId, syncOffsetNs),
                      requestId,
                      protocol::ResponseKind::sessionStateChanged,
                      response))
            return false;
        lastSentSyncOffsetNs = syncOffsetNs;
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

bool ServiceClient::synchronizePreviewState()
{
    if (lastSentSourceId.empty())
        return true;

    const auto previewEnabled = previewEnabledState.load(std::memory_order_acquire);
    if (previewEnabled == lastSentPreviewEnabled)
        return true;

    const auto requestId = nextRequestId();
    if (previewEnabled)
    {
        if (!sendMessage(protocol::makeRequestPreview(requestId, runtimeInstanceId, 480U, 270U, 30U)))
            return false;
    }
    else
    {
        if (!sendMessage(protocol::makeStopPreview(requestId, runtimeInstanceId)))
            return false;
    }

    lastSentPreviewEnabled = previewEnabled;
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

bool ServiceClient::readExact(void* dest, int bytesToRead, int timeoutMs)
{
    auto* buffer = static_cast<char*>(dest);
    int totalRead = 0;
    const auto startTime = juce::Time::getMillisecondCounter();

    while (totalRead < bytesToRead)
    {
        if (threadShouldExit() || receiver.threadShouldExit())
            return false;

        const auto elapsed = static_cast<int>(juce::Time::getMillisecondCounter() - startTime);
        if (elapsed >= timeoutMs && timeoutMs > 0)
            return false;

        const auto remainingTimeout = (timeoutMs > 0) ? std::max(1, timeoutMs - elapsed) : 0;
        const auto bytesRemaining = bytesToRead - totalRead;
        const auto numRead = pipe.read(buffer + totalRead, bytesRemaining, remainingTimeout);
        if (numRead <= 0)
            return false;
        totalRead += numRead;
    }
    return totalRead == bytesToRead;
}

bool ServiceClient::writeExact(const void* src, int bytesToWrite, int timeoutMs)
{
    const auto* buffer = static_cast<const char*>(src);
    int totalWritten = 0;
    const auto startTime = juce::Time::getMillisecondCounter();

    while (totalWritten < bytesToWrite)
    {
        if (threadShouldExit())
            return false;

        const auto elapsed = static_cast<int>(juce::Time::getMillisecondCounter() - startTime);
        if (elapsed >= timeoutMs && timeoutMs > 0)
            return false;

        const auto remainingTimeout = (timeoutMs > 0) ? std::max(1, timeoutMs - elapsed) : 0;
        const auto bytesRemaining = bytesToWrite - totalWritten;
        const auto numWritten = pipe.write(buffer + totalWritten, bytesRemaining, remainingTimeout);
        if (numWritten <= 0)
            return false;
        totalWritten += numWritten;
    }
    return totalWritten == bytesToWrite;
}

bool ServiceClient::sendMessage(const protocol::Bytes& message)
{
    if (message.size() > maximumMessageBytes)
        return false;

    const auto length = static_cast<std::uint32_t>(message.size());
    std::vector<std::uint8_t> packet(4 + message.size());
    packet[0] = static_cast<std::uint8_t>(length & 0xffU);
    packet[1] = static_cast<std::uint8_t>((length >> 8U) & 0xffU);
    packet[2] = static_cast<std::uint8_t>((length >> 16U) & 0xffU);
    packet[3] = static_cast<std::uint8_t>((length >> 24U) & 0xffU);
    std::memcpy(packet.data() + 4, message.data(), message.size());

    return writeExact(packet.data(), static_cast<int>(packet.size()), ioTimeoutMilliseconds);
}

bool ServiceClient::receiveResponse(protocol::Response& response, int timeoutMilliseconds)
{
    std::array<std::uint8_t, 4> header{};
    if (!readExact(header.data(), static_cast<int>(header.size()), timeoutMilliseconds))
        return false;

    const auto length = static_cast<std::uint32_t>(header[0])
                        | (static_cast<std::uint32_t>(header[1]) << 8U)
                        | (static_cast<std::uint32_t>(header[2]) << 16U)
                        | (static_cast<std::uint32_t>(header[3]) << 24U);
    if (length == 0U || length > maximumMessageBytes)
        return false;

    protocol::Bytes payload(length);
    if (!readExact(payload.data(), static_cast<int>(payload.size()), ioTimeoutMilliseconds))
        return false;
    return protocol::decodeResponse(payload, response);
}

bool ServiceClient::exchange(const protocol::Bytes& message,
                             std::uint64_t requestId,
                             protocol::ResponseKind expectedKind,
                             protocol::Response& response)
{
    auto req = std::make_shared<PendingRequest>();
    req->requestId = requestId;
    {
        const juce::ScopedLock lock(pendingLock);
        pendingRequests[requestId] = req;
    }

    if (!sendMessage(message))
    {
        const juce::ScopedLock lock(pendingLock);
        pendingRequests.erase(requestId);
        return false;
    }

    const bool signaled = req->event.wait(ioTimeoutMilliseconds);

    {
        const juce::ScopedLock lock(pendingLock);
        pendingRequests.erase(requestId);
    }

    if (!signaled || !req->ready)
        return false;

    response = std::move(req->response);
    if (response.kind == protocol::ResponseKind::error)
    {
        setStatus(ServiceConnectionState::error,
                  juce::String(response.errorCode) + ": " + juce::String(response.message));
        return false;
    }

    return response.kind == expectedKind;
}

void ServiceClient::storeFinalizedTake(const protocol::Response& response)
{
    const juce::ScopedLock lock(finalizedTakeLock);
    finalizedTake.takeId = juce::String::fromUTF8(response.takeId.c_str());
    finalizedTake.videoPath = juce::String::fromUTF8(response.videoPath.c_str());
    finalizedTake.metadataPath = juce::String::fromUTF8(response.metadataPath.c_str());
    ++finalizedTake.revision;
}

void ServiceClient::storePreviewFrame(const protocol::Response& response)
{
    const juce::ScopedLock lock(previewLock);
    currentPreviewFrame.width = response.width;
    currentPreviewFrame.height = response.height;
    currentPreviewFrame.format = juce::String::fromUTF8(response.format.c_str());
    currentPreviewFrame.frameData = response.frameData;
    currentPreviewFrame.timestampNs = response.timestampNs;
    ++currentPreviewFrame.revision;
}

void ServiceClient::disconnect()
{
    receiver.stopThread(1000);

    {
        const juce::ScopedLock lock(pendingLock);
        for (auto& pair : pendingRequests)
        {
            pair.second->event.signal();
        }
        pendingRequests.clear();
    }

    if (pipe.isOpen() && !runtimeInstanceId.empty())
        static_cast<void>(sendMessage(protocol::makeUnregisterInstance(nextRequestId(), runtimeInstanceId)));
    pipe.close();
    runtimeInstanceId.clear();
    setStatus(ServiceConnectionState::stopped, "Disconnected from recording service");
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
