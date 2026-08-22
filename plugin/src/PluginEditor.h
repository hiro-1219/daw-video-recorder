// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "PluginProcessor.h"

#include <juce_gui_extra/juce_gui_extra.h>

#include <vector>

namespace dawvr
{
class PluginEditor final : public juce::AudioProcessorEditor, private juce::Timer
{
public:
    explicit PluginEditor(PluginProcessor& processorToUse);
    ~PluginEditor() override;

    void paint(juce::Graphics&) override;
    void resized() override;

private:
    void timerCallback() override;
    void chooseProjectRoot();
    void updateSourceSelector();

    PluginProcessor& processor;
    juce::Label titleLabel;
    juce::ToggleButton recordArmButton { "Record Arm" };
    juce::Label sourceLabel;
    juce::ComboBox sourceSelector;
    juce::Label projectRootLabel;
    juce::TextEditor projectRootEditor;
    juce::TextButton browseButton { "Browse..." };
    juce::Label encodingLabel;
    juce::ComboBox encodingSelector;
    juce::ToggleButton hardwareEncoderButton { "Prefer hardware encoder" };
    juce::Label syncOffsetLabel;
    juce::Slider syncOffsetSlider;
    juce::Label serviceStatusLabel;
    juce::Label metricsLabel;
    juce::Label lastTakeLabel;
    std::unique_ptr<juce::FileChooser> fileChooser;
    std::unique_ptr<juce::AudioProcessorValueTreeState::ButtonAttachment> armAttachment;
    std::vector<protocol::CameraSource> displayedSources;
    std::uint64_t displayedSourceRevision{};
    std::uint64_t displayedTakeRevision{};

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(PluginEditor)
};
} // namespace dawvr
