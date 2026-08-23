// SPDX-License-Identifier: AGPL-3.0-only
#include "PluginProcessor.h"
#include "PluginEditor.h"

#include <chrono>
#include <cmath>

namespace dawvr
{
namespace
{
constexpr auto stateTreeName = "DAWVideoRecorderState";
constexpr auto recordArmParameterId = "recordArm";
constexpr auto instanceIdProperty = "persistentInstanceId";
constexpr auto projectRootProperty = "projectRoot";
constexpr auto selectedSourceProperty = "selectedSourceId";
constexpr auto encodingPresetProperty = "encodingPreset";
constexpr auto hardwareEncodingProperty = "hardwareEncodingPreferred";
constexpr auto syncOffsetProperty = "syncOffsetNs";

std::uint32_t bitRateForPreset(int preset)
{
    switch (preset)
    {
        case 1: return 4'000'000U;
        case 3: return 16'000'000U;
        default: return 8'000'000U;
    }
}
} // namespace

PluginProcessor::PluginProcessor()
    : AudioProcessor(BusesProperties()
                         .withInput("Input", juce::AudioChannelSet::stereo(), true)
                         .withOutput("Output", juce::AudioChannelSet::stereo(), true)),
      state(*this, nullptr, stateTreeName, createParameterLayout()),
      instanceId(juce::Uuid().toString()),
      serviceClient(transportQueue, recordArmState)
{
    state.addParameterListener(recordArmParameterId, this);
    serviceClient.setIdentity(instanceId, "Camera");
    serviceClient.setPreviewEnabled(true);
    serviceClient.start();
}

PluginProcessor::~PluginProcessor()
{
    state.removeParameterListener(recordArmParameterId, this);
    serviceClient.stop();
}

void PluginProcessor::prepareToPlay(double, int)
{
    audioProcessingActive.store(true, std::memory_order_release);
    serviceClient.start();
}

void PluginProcessor::releaseResources()
{
    audioProcessingActive.store(false, std::memory_order_release);
}

bool PluginProcessor::isBusesLayoutSupported(const BusesLayout& layouts) const
{
    const auto input = layouts.getMainInputChannelSet();
    const auto output = layouts.getMainOutputChannelSet();
    return input == output && (output == juce::AudioChannelSet::mono()
                               || output == juce::AudioChannelSet::stereo());
}

void PluginProcessor::processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer&)
{
    process(buffer);
}

void PluginProcessor::processBlock(juce::AudioBuffer<double>& buffer, juce::MidiBuffer&)
{
    process(buffer);
}

template <typename Sample>
void PluginProcessor::process(juce::AudioBuffer<Sample>& buffer)
{
    juce::ScopedNoDenormals noDenormals;
    static_cast<void>(buffer); // Audio is deliberately passed through unchanged.
    audioProcessingActive.store(true, std::memory_order_relaxed);

    auto snapshot = captureTransportSnapshot();
    const auto isPlaying = (snapshot.flags & TransportFlags::playing) != 0U;
    const auto isRecording = (snapshot.flags & TransportFlags::recording) != 0U;
    dawPlayingState.store(isPlaying, std::memory_order_relaxed);
    dawRecordingState.store(isRecording, std::memory_order_relaxed);

    if ((snapshot.flags & TransportFlags::projectSampleValid) != 0U)
    {
        latestProjectSample.store(snapshot.projectSamplePosition, std::memory_order_relaxed);
        samplePositionValid.store(true, std::memory_order_relaxed);
    }
    else
    {
        samplePositionValid.store(false, std::memory_order_relaxed);
    }

    if (!transportQueue.push(snapshot))
        droppedSnapshots.fetch_add(1U, std::memory_order_relaxed);
}

TransportSnapshot PluginProcessor::captureTransportSnapshot() noexcept
{
    TransportSnapshot snapshot;
    snapshot.sequenceNumber = sequenceNumber.fetch_add(1U, std::memory_order_relaxed) + 1U;
    snapshot.sampleRate = getSampleRate();
    snapshot.callbackMonotonicTimeNs = std::chrono::duration_cast<std::chrono::nanoseconds>(
                                           std::chrono::steady_clock::now().time_since_epoch())
                                           .count();

    if (isNonRealtime())
        snapshot.flags |= TransportFlags::offlineRendering;

    if (const auto* hostPlayHead = getPlayHead())
    {
        if (const auto position = hostPlayHead->getPosition())
        {
            if (position->getIsPlaying())
                snapshot.flags |= TransportFlags::playing;
            if (position->getIsRecording())
                snapshot.flags |= TransportFlags::recording;
            if (position->getLoopPoints().hasValue())
                snapshot.flags |= TransportFlags::looping;

            if (const auto samples = position->getTimeInSamples())
            {
                snapshot.projectSamplePosition = *samples;
                snapshot.flags |= TransportFlags::projectSampleValid;
            }

            if (const auto hostTime = position->getHostTimeNs())
            {
                snapshot.hostTimeNs = *hostTime;
                snapshot.flags |= TransportFlags::hostTimeValid;
            }

            if (const auto bpm = position->getBpm())
            {
                snapshot.tempo = *bpm;
                snapshot.flags |= TransportFlags::tempoValid;
            }

            if (const auto signature = position->getTimeSignature())
            {
                snapshot.timeSignatureNumerator = signature->numerator;
                snapshot.timeSignatureDenominator = signature->denominator;
                snapshot.flags |= TransportFlags::timeSignatureValid;
            }

            if (const auto ppq = position->getPpqPosition())
            {
                snapshot.ppqPosition = *ppq;
                snapshot.flags |= TransportFlags::ppqValid;
            }
        }
    }

    return snapshot;
}

juce::AudioProcessorEditor* PluginProcessor::createEditor()
{
    return new PluginEditor(*this);
}

bool PluginProcessor::hasEditor() const { return true; }
const juce::String PluginProcessor::getName() const { return "DAW Video Recorder"; }
bool PluginProcessor::acceptsMidi() const { return false; }
bool PluginProcessor::producesMidi() const { return false; }
bool PluginProcessor::isMidiEffect() const { return false; }
double PluginProcessor::getTailLengthSeconds() const { return 0.0; }
int PluginProcessor::getNumPrograms() { return 1; }
int PluginProcessor::getCurrentProgram() { return 0; }
void PluginProcessor::setCurrentProgram(int) {}
const juce::String PluginProcessor::getProgramName(int) { return {}; }
void PluginProcessor::changeProgramName(int, const juce::String&) {}

void PluginProcessor::getStateInformation(juce::MemoryBlock& destinationData)
{
    auto tree = state.copyState();
    tree.setProperty(instanceIdProperty, instanceId, nullptr);
    tree.setProperty(projectRootProperty, selectedProjectRoot, nullptr);
    tree.setProperty(selectedSourceProperty, selectedCameraSourceId, nullptr);
    tree.setProperty(encodingPresetProperty, selectedEncodingPreset, nullptr);
    tree.setProperty(hardwareEncodingProperty, hardwareEncodingPreferred, nullptr);
    tree.setProperty(syncOffsetProperty, configuredSyncOffsetNs, nullptr);
    if (const auto xml = tree.createXml())
        copyXmlToBinary(*xml, destinationData);
}

void PluginProcessor::setStateInformation(const void* data, int sizeInBytes)
{
    const auto xml = getXmlFromBinary(data, sizeInBytes);
    if (xml == nullptr || !xml->hasTagName(state.state.getType()))
        return;

    const auto tree = juce::ValueTree::fromXml(*xml);
    state.replaceState(tree);
    instanceId = tree.getProperty(instanceIdProperty, juce::Uuid().toString()).toString();
    selectedProjectRoot = tree.getProperty(projectRootProperty).toString();
    selectedCameraSourceId = tree.getProperty(selectedSourceProperty).toString();
    selectedEncodingPreset = juce::jlimit(1, 3, static_cast<int>(tree.getProperty(encodingPresetProperty, 2)));
    hardwareEncodingPreferred = static_cast<bool>(tree.getProperty(hardwareEncodingProperty, true));
    configuredSyncOffsetNs = static_cast<juce::int64>(tree.getProperty(syncOffsetProperty, 0));
    if (const auto* arm = state.getRawParameterValue(recordArmParameterId))
        recordArmState.store(arm->load() >= 0.5F, std::memory_order_release);
    serviceClient.setIdentity(instanceId, "Camera");
    serviceClient.setProjectRoot(selectedProjectRoot);
    serviceClient.setSelectedSource(selectedCameraSourceId);
    serviceClient.setEncodingProfile(bitRateForPreset(selectedEncodingPreset),
                                     2'000U,
                                     hardwareEncodingPreferred);
    serviceClient.setSyncOffsetNs(configuredSyncOffsetNs);
    takeManager.refreshTakes(selectedProjectRoot, instanceId);
}

juce::AudioProcessorValueTreeState& PluginProcessor::parameters() noexcept { return state; }
const juce::String& PluginProcessor::persistentInstanceId() const noexcept { return instanceId; }
const juce::String& PluginProcessor::projectRoot() const noexcept { return selectedProjectRoot; }

void PluginProcessor::setProjectRoot(juce::String newProjectRoot)
{
    selectedProjectRoot = std::move(newProjectRoot);
    serviceClient.setProjectRoot(selectedProjectRoot);
    takeManager.refreshTakes(selectedProjectRoot, instanceId);
}

const juce::String& PluginProcessor::selectedSourceId() const noexcept
{
    return selectedCameraSourceId;
}

void PluginProcessor::setSelectedSourceId(juce::String sourceId)
{
    selectedCameraSourceId = std::move(sourceId);
    serviceClient.setSelectedSource(selectedCameraSourceId);
}

std::vector<protocol::CameraSource> PluginProcessor::availableSources() const
{
    return serviceClient.availableSources();
}

std::uint64_t PluginProcessor::sourceListRevision() const noexcept
{
    return serviceClient.sourceListRevision();
}

void PluginProcessor::requestSourceRefresh() noexcept
{
    serviceClient.requestSourceRefresh();
}

int PluginProcessor::encodingPreset() const noexcept
{
    return selectedEncodingPreset;
}

void PluginProcessor::setEncodingPreset(int preset)
{
    selectedEncodingPreset = juce::jlimit(1, 3, preset);
    serviceClient.setEncodingProfile(bitRateForPreset(selectedEncodingPreset),
                                     2'000U,
                                     hardwareEncodingPreferred);
}

bool PluginProcessor::prefersHardwareEncoding() const noexcept
{
    return hardwareEncodingPreferred;
}

void PluginProcessor::setPrefersHardwareEncoding(bool preferred)
{
    hardwareEncodingPreferred = preferred;
    serviceClient.setEncodingProfile(bitRateForPreset(selectedEncodingPreset),
                                     2'000U,
                                     hardwareEncodingPreferred);
}

double PluginProcessor::syncOffsetMilliseconds() const noexcept
{
    return static_cast<double>(configuredSyncOffsetNs) / 1'000'000.0;
}

void PluginProcessor::setSyncOffsetMilliseconds(double milliseconds)
{
    const auto limited = juce::jlimit(-5'000.0, 5'000.0, milliseconds);
    configuredSyncOffsetNs = static_cast<std::int64_t>(std::llround(limited * 1'000'000.0));
    serviceClient.setSyncOffsetNs(configuredSyncOffsetNs);
}

void PluginProcessor::setPreviewEnabled(bool enabled)
{
    previewEnabledFlag.store(enabled, std::memory_order_release);
    serviceClient.setPreviewEnabled(enabled);
}

ServiceClient::PreviewFrameData PluginProcessor::latestPreviewFrame() const
{
    return serviceClient.latestPreviewFrame();
}

bool PluginProcessor::isArmed() const noexcept
{
    return recordArmState.load(std::memory_order_acquire);
}

std::uint64_t PluginProcessor::droppedSnapshotCount() const noexcept
{
    return droppedSnapshots.load(std::memory_order_relaxed);
}

std::uint64_t PluginProcessor::sentSnapshotCount() const noexcept
{
    return serviceClient.sentSnapshotCount();
}

bool PluginProcessor::isTransportWorkerRunning() const noexcept
{
    return serviceClient.isWorkerRunning();
}

ServiceConnectionState PluginProcessor::serviceConnectionState() const noexcept
{
    return serviceClient.connectionState();
}

juce::String PluginProcessor::serviceStatusText() const
{
    return serviceClient.statusText();
}

FinalizedTakeInfo PluginProcessor::latestFinalizedTake() const
{
    return serviceClient.latestFinalizedTake();
}

void PluginProcessor::parameterChanged(const juce::String& parameterId, float newValue)
{
    if (parameterId == recordArmParameterId)
        recordArmState.store(newValue >= 0.5F, std::memory_order_release);
}

juce::AudioProcessorValueTreeState::ParameterLayout PluginProcessor::createParameterLayout()
{
    juce::AudioProcessorValueTreeState::ParameterLayout layout;
    layout.add(std::make_unique<juce::AudioParameterBool>(
        juce::ParameterID { recordArmParameterId, 1 }, "Record Arm", false));
    return layout;
}
} // namespace dawvr

juce::AudioProcessor* JUCE_CALLTYPE createPluginFilter()
{
    return new dawvr::PluginProcessor();
}
