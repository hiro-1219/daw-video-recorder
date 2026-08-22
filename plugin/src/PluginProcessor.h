// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "ServiceClient.h"
#include "SpscQueue.h"
#include "TransportSnapshot.h"

#include <juce_audio_processors/juce_audio_processors.h>

#include <atomic>

namespace dawvr
{
class PluginProcessor final : public juce::AudioProcessor,
                              private juce::AudioProcessorValueTreeState::Listener
{
public:
    PluginProcessor();
    ~PluginProcessor() override;

    void prepareToPlay(double sampleRate, int maximumExpectedSamplesPerBlock) override;
    void releaseResources() override;
    bool isBusesLayoutSupported(const BusesLayout& layouts) const override;
    void processBlock(juce::AudioBuffer<float>&, juce::MidiBuffer&) override;
    void processBlock(juce::AudioBuffer<double>&, juce::MidiBuffer&) override;

    juce::AudioProcessorEditor* createEditor() override;
    bool hasEditor() const override;

    const juce::String getName() const override;
    bool acceptsMidi() const override;
    bool producesMidi() const override;
    bool isMidiEffect() const override;
    double getTailLengthSeconds() const override;

    int getNumPrograms() override;
    int getCurrentProgram() override;
    void setCurrentProgram(int) override;
    const juce::String getProgramName(int) override;
    void changeProgramName(int, const juce::String&) override;

    void getStateInformation(juce::MemoryBlock& destinationData) override;
    void setStateInformation(const void* data, int sizeInBytes) override;

    [[nodiscard]] juce::AudioProcessorValueTreeState& parameters() noexcept;
    [[nodiscard]] const juce::String& persistentInstanceId() const noexcept;
    [[nodiscard]] const juce::String& projectRoot() const noexcept;
    void setProjectRoot(juce::String newProjectRoot);
    [[nodiscard]] const juce::String& selectedSourceId() const noexcept;
    void setSelectedSourceId(juce::String sourceId);
    [[nodiscard]] std::vector<protocol::CameraSource> availableSources() const;
    [[nodiscard]] std::uint64_t sourceListRevision() const noexcept;
    void requestSourceRefresh() noexcept;

    [[nodiscard]] std::uint64_t droppedSnapshotCount() const noexcept;
    [[nodiscard]] std::uint64_t sentSnapshotCount() const noexcept;
    [[nodiscard]] bool isTransportWorkerRunning() const noexcept;
    [[nodiscard]] ServiceConnectionState serviceConnectionState() const noexcept;
    [[nodiscard]] juce::String serviceStatusText() const;
    [[nodiscard]] FinalizedTakeInfo latestFinalizedTake() const;

private:
    template <typename Sample>
    void process(juce::AudioBuffer<Sample>& buffer);

    [[nodiscard]] TransportSnapshot captureTransportSnapshot() noexcept;
    static juce::AudioProcessorValueTreeState::ParameterLayout createParameterLayout();
    void parameterChanged(const juce::String& parameterId, float newValue) override;

    juce::AudioProcessorValueTreeState state;
    juce::String instanceId;
    juce::String selectedProjectRoot;
    juce::String selectedCameraSourceId;

    std::atomic<bool> recordArmState{};
    SpscQueue<TransportSnapshot, 2048U> transportQueue;
    ServiceClient serviceClient;
    std::atomic<std::uint64_t> sequenceNumber{};
    std::atomic<std::uint64_t> droppedSnapshots{};

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(PluginProcessor)
};
} // namespace dawvr
