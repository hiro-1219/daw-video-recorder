// SPDX-License-Identifier: AGPL-3.0-only
#include "PluginEditor.h"

namespace dawvr
{
PluginEditor::PluginEditor(PluginProcessor& processorToUse)
    : AudioProcessorEditor(processorToUse), processor(processorToUse)
{
    titleLabel.setText("DAW Video Recorder", juce::dontSendNotification);
    titleLabel.setFont(juce::FontOptions(22.0F, juce::Font::bold));
    addAndMakeVisible(titleLabel);

    armAttachment = std::make_unique<juce::AudioProcessorValueTreeState::ButtonAttachment>(
        processor.parameters(), "recordArm", recordArmButton);
    addAndMakeVisible(recordArmButton);

    sourceLabel.setText("Camera", juce::dontSendNotification);
    addAndMakeVisible(sourceLabel);
    sourceSelector.addItem("Camera enumeration pending", 1);
    sourceSelector.setSelectedId(1);
    sourceSelector.setEnabled(false);
    addAndMakeVisible(sourceSelector);

    projectRootLabel.setText("Project Root", juce::dontSendNotification);
    addAndMakeVisible(projectRootLabel);
    projectRootEditor.setText(processor.projectRoot(), false);
    projectRootEditor.onFocusLost = [this] { processor.setProjectRoot(projectRootEditor.getText()); };
    addAndMakeVisible(projectRootEditor);

    browseButton.onClick = [this] { chooseProjectRoot(); };
    addAndMakeVisible(browseButton);

    serviceStatusLabel.setColour(juce::Label::textColourId, juce::Colours::orange);
    addAndMakeVisible(serviceStatusLabel);
    addAndMakeVisible(metricsLabel);

    setResizable(true, true);
    setResizeLimits(480, 260, 1000, 700);
    setSize(620, 320);
    startTimerHz(4);
}

PluginEditor::~PluginEditor()
{
    stopTimer();
}

void PluginEditor::paint(juce::Graphics& graphics)
{
    graphics.fillAll(juce::Colour::fromRGB(28, 30, 34));
}

void PluginEditor::resized()
{
    auto area = getLocalBounds().reduced(20);
    titleLabel.setBounds(area.removeFromTop(38));
    area.removeFromTop(8);
    recordArmButton.setBounds(area.removeFromTop(32));
    area.removeFromTop(8);

    auto sourceRow = area.removeFromTop(32);
    sourceLabel.setBounds(sourceRow.removeFromLeft(100));
    sourceSelector.setBounds(sourceRow);
    area.removeFromTop(8);

    auto rootRow = area.removeFromTop(32);
    projectRootLabel.setBounds(rootRow.removeFromLeft(100));
    browseButton.setBounds(rootRow.removeFromRight(100));
    rootRow.removeFromRight(8);
    projectRootEditor.setBounds(rootRow);
    area.removeFromTop(16);

    serviceStatusLabel.setBounds(area.removeFromTop(28));
    metricsLabel.setBounds(area.removeFromTop(28));
}

void PluginEditor::timerCallback()
{
    const auto workerRunning = processor.isTransportWorkerRunning();
    serviceStatusLabel.setText(
        workerRunning ? "Transport worker: running (Named Pipe connection pending)"
                      : "Transport worker: stopped",
        juce::dontSendNotification);
    serviceStatusLabel.setColour(
        juce::Label::textColourId, workerRunning ? juce::Colours::orange : juce::Colours::red);
    metricsLabel.setText(
        "Dropped transport snapshots: " + juce::String(processor.droppedSnapshotCount()),
        juce::dontSendNotification);
}

void PluginEditor::chooseProjectRoot()
{
    auto initial = processor.projectRoot().isNotEmpty() ? juce::File(processor.projectRoot())
                                                        : juce::File::getSpecialLocation(
                                                              juce::File::userDocumentsDirectory);
    fileChooser = std::make_unique<juce::FileChooser>("Select Studio One project root", initial);
    fileChooser->launchAsync(juce::FileBrowserComponent::openMode
                                 | juce::FileBrowserComponent::canSelectDirectories,
                             [this](const juce::FileChooser& chooser) {
                                 const auto result = chooser.getResult();
                                 if (result.isDirectory())
                                 {
                                     projectRootEditor.setText(result.getFullPathName());
                                     processor.setProjectRoot(result.getFullPathName());
                                 }
                             });
}
} // namespace dawvr
