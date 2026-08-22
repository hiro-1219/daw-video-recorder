// SPDX-License-Identifier: AGPL-3.0-only
#include "PluginProcessor.h"
#include "PluginEditor.h"

#include <chrono>

namespace dawvr
{
namespace
{
constexpr auto stateTreeName = "DAWVideoRecorderState";
constexpr auto recordArmParameterId = "recordArm";
constexpr auto instanceIdProperty = "persistentInstanceId";
constexpr auto projectRootProperty = "projectRoot";
constexpr auto selectedSourceProperty = "selectedSourceId";
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
}

PluginProcessor::~PluginProcessor()
{
    state.removeParameterListener(recordArmParameterId, this);
    serviceClient.stop();
}

void PluginProcessor::prepareToPlay(double, int)
{
    serviceClient.start();
}

void PluginProcessor::releaseResources()
{
    serviceClient.stop();
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

    auto snapshot = captureTransportSnapshot();
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

            if (const auto tempo = position->getBpm())
            {
                snapshot.tempo = *tempo;
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

bool PluginProcessor::hasEditor() const
{
    return true;
}

const juce::String PluginProcessor::getName() const
{
    return JucePlugin_Name;
}

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
    if (const auto* arm = state.getRawParameterValue(recordArmParameterId))
        recordArmState.store(arm->load() >= 0.5F, std::memory_order_release);
    serviceClient.setIdentity(instanceId, "Camera");
    serviceClient.setProjectRoot(selectedProjectRoot);
    serviceClient.setSelectedSource(selectedCameraSourceId);
}

juce::AudioProcessorValueTreeState& PluginProcessor::parameters() noexcept { return state; }
const juce::String& PluginProcessor::persistentInstanceId() const noexcept { return instanceId; }
const juce::String& PluginProcessor::projectRoot() const noexcept { return selectedProjectRoot; }

void PluginProcessor::setProjectRoot(juce::String newProjectRoot)
{
    selectedProjectRoot = std::move(newProjectRoot);
    serviceClient.setProjectRoot(selectedProjectRoot);
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
