// SPDX-License-Identifier: AGPL-3.0-only
#include "ServiceClient.h"

#include <array>
#include <memory>
#include <vector>

namespace dawvr
{
namespace
{
constexpr auto pipeName = "daw-video-recorder-v2";
constexpr auto maximumMessageBytes = 1024U * 1024U;
constexpr auto ioTimeoutMilliseconds = 5000;
constexpr auto maximumBatchSize = 64U;
constexpr auto sourceRefreshIntervalMilliseconds = 2000;

juce::CriticalSection serviceProcessLock;
std::unique_ptr<juce::ChildProcess> serviceProcess;
std::int64_t nextLaunchAttemptTimeMs{};

std::vector<juce::File> serviceExecutableCandidates()
{
    std::vector<juce::File> candidates;
    const auto overridePath = juce::SystemStats::getEnvironmentVariable("DAWVR_SERVICE_PATH", {});
    if (overridePath.isNotEmpty())
        candidates.emplace_back(overridePath);

    const auto localAppData = juce::SystemStats::getEnvironmentVariable("LOCALAPPDATA", {});
    if (localAppData.isNotEmpty())
        candidates.push_back(juce::File(localAppData)
                                 .getChildFile("DAW Video Recorder")
                                 .getChildFile("daw-video-recorder-service.exe"));

    const auto workingDirectory = juce::File::getCurrentWorkingDirectory();
    candidates.push_back(workingDirectory.getChildFile("target")
                             .getChildFile("debug")
                             .getChildFile("daw-video-recorder-service.exe"));
    candidates.push_back(workingDirectory.getChildFile("target")
                             .getChildFile("release")
                             .getChildFile("daw-video-recorder-service.exe"));
    return candidates;
}

bool tryLaunchRecordingService()
{
    const juce::ScopedLock lock(serviceProcessLock);
    if (serviceProcess != nullptr && serviceProcess->isRunning())
        return true;

    const auto now = juce::Time::currentTimeMillis();
    if (now < nextLaunchAttemptTimeMs)
        return false;
    nextLaunchAttemptTimeMs = now + 2000;

    for (const auto& candidate : serviceExecutableCandidates())
    {
        if (!candidate.existsAsFile())
            continue;
        auto process = std::make_unique<juce::ChildProcess>();
        juce::StringArray arguments { candidate.getFullPathName() };
        if (process->start(arguments, 0))
        {
            serviceProcess = std::move(process);
            return true;
        }
    }
    return false;
}
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

void ServiceClient::setSelectedSource(juce::String sourceId)
{
    const juce::ScopedLock lock(configurationLock);
    if (configuredSourceId == sourceId)
        return;
    configuredSourceId = std::move(sourceId);
    configurationRevision.fetch_add(1U, std::memory_order_release);
    notify();
}

void ServiceClient::setEncodingProfile(std::uint32_t bitRate,
                                       std::uint32_t gopDurationMs,
                                       bool preferHardware)
{
    const juce::ScopedLock lock(configurationLock);
    if (configuredBitRate == bitRate && configuredGopDurationMs == gopDurationMs
        && configuredPreferHardware == preferHardware)
        return;
    configuredBitRate = bitRate;
    configuredGopDurationMs = gopDurationMs;
    configuredPreferHardware = preferHardware;
    configurationRevision.fetch_add(1U, std::memory_order_release);
    notify();
}

void ServiceClient::setSyncOffsetNs(std::int64_t offsetNs)
{
    const juce::ScopedLock lock(configurationLock);
    if (configuredSyncOffsetNs == offsetNs)
        return;
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

        pollIncomingMessages();
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
        const auto launched = tryLaunchRecordingService();
        setStatus(launched ? ServiceConnectionState::connecting : ServiceConnectionState::error,
                  launched ? "Recording service started; connecting..."
                           : "Recording service executable was not found");
        return false;
    }

    protocol::Response response;
    auto requestId = nextRequestId();
    if (!exchange(protocol::makeHello(requestId),
                  requestId,
                  protocol::ResponseKind::helloAck,
                  response)
        || response.selectedVersion != protocol::currentProtocolVersion)
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
    lastSentSourceId.clear();
    lastSentBitRate = 0U;
    lastSentGopDurationMs = 0U;
    lastSentPreferHardware = false;
    lastSentSyncOffsetNs = std::numeric_limits<std::int64_t>::min();
    lastSentArmed = false;
    lastSentPreviewEnabled = false;
    nextSourceRefreshTimeMs = 0;
    sourceRefreshRequested.store(true, std::memory_order_release);
    setStatus(ServiceConnectionState::connected,
              "Connected to recording service " + juce::String(serviceVersion));
    return true;
}

bool ServiceClient::refreshSources()
{
    protocol::Response response;
    const auto requestId = nextRequestId();
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
    nextSourceRefreshTimeMs = juce::Time::currentTimeMillis() + sourceRefreshIntervalMilliseconds;
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

void ServiceClient::pollIncomingMessages()
{
    for (auto i = 0; i < 8; ++i)
    {
        protocol::Response response;
        if (!receiveResponse(response, 0))
            break;
        if (response.kind == protocol::ResponseKind::recordingFinalized)
            storeFinalizedTake(response);
        else if (response.kind == protocol::ResponseKind::previewFrame)
            storePreviewFrame(response);
    }
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
    if (pipe.read(payload.data(), static_cast<int>(payload.size()), ioTimeoutMilliseconds)
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

    for (auto remainingResponses = 0; remainingResponses < 32; ++remainingResponses)
    {
        if (!receiveResponse(response, ioTimeoutMilliseconds))
            return false;
        if (response.kind == protocol::ResponseKind::error)
        {
            setStatus(ServiceConnectionState::error,
                      juce::String(response.errorCode) + ": " + juce::String(response.message));
            return false;
        }
        if (response.kind == protocol::ResponseKind::recordingFinalized)
        {
            storeFinalizedTake(response);
            continue;
        }
        if (response.kind == protocol::ResponseKind::previewFrame)
        {
            storePreviewFrame(response);
            continue;
        }
        if (response.requestId == requestId && response.kind == expectedKind)
            return true;
    }
    return false;
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
    if (pipe.isOpen() && !runtimeInstanceId.empty())
        static_cast<void>(sendMessage(protocol::makeUnregisterInstance(nextRequestId(), runtimeInstanceId)));
    pipe.close();
    runtimeInstanceId.clear();
    if (!threadShouldExit()
        && currentState.load(std::memory_order_acquire) != ServiceConnectionState::error)
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
