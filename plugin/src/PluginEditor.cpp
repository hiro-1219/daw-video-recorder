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
    sourceSelector.addItem("Searching for cameras...", 1);
    sourceSelector.setSelectedId(1, juce::dontSendNotification);
    sourceSelector.setEnabled(false);
    sourceSelector.onChange = [this] {
        const auto index = sourceSelector.getSelectedId() - 1;
        if (index >= 0 && static_cast<std::size_t>(index) < displayedSources.size())
            processor.setSelectedSourceId(
                juce::String::fromUTF8(
                    displayedSources[static_cast<std::size_t>(index)].sourceId.c_str()));
    };
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
    lastTakeLabel.setColour(juce::Label::textColourId, juce::Colours::lightgrey);
    addAndMakeVisible(lastTakeLabel);

    setResizable(true, true);
    setResizeLimits(480, 300, 1000, 700);
    setSize(620, 360);
    processor.requestSourceRefresh();
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
    lastTakeLabel.setBounds(area.removeFromTop(48));
}

void PluginEditor::timerCallback()
{
    updateSourceSelector();
    const auto connectionState = processor.serviceConnectionState();
    serviceStatusLabel.setText(processor.serviceStatusText(), juce::dontSendNotification);
    const auto colour = connectionState == ServiceConnectionState::connected
                            ? juce::Colours::lightgreen
                            : (connectionState == ServiceConnectionState::error ? juce::Colours::red
                                                                                : juce::Colours::orange);
    serviceStatusLabel.setColour(juce::Label::textColourId, colour);
    metricsLabel.setText(
        "Transport sent: " + juce::String(processor.sentSnapshotCount())
            + " / queue drops: " + juce::String(processor.droppedSnapshotCount()),
        juce::dontSendNotification);

    const auto latestTake = processor.latestFinalizedTake();
    if (latestTake.revision != 0U && latestTake.revision != displayedTakeRevision)
    {
        displayedTakeRevision = latestTake.revision;
        lastTakeLabel.setText("Last take: " + latestTake.videoPath,
                              juce::dontSendNotification);
        lastTakeLabel.setTooltip(latestTake.metadataPath);
    }
}

void PluginEditor::updateSourceSelector()
{
    const auto revision = processor.sourceListRevision();
    if (revision == 0U || revision == displayedSourceRevision)
        return;

    const auto available = processor.availableSources();
    displayedSourceRevision = revision;
    displayedSources = available;
    sourceSelector.clear(juce::dontSendNotification);
    if (displayedSources.empty())
    {
        sourceSelector.addItem("No camera detected", 1);
        sourceSelector.setSelectedId(1, juce::dontSendNotification);
        sourceSelector.setEnabled(false);
        return;
    }

    const auto selectedSource = processor.selectedSourceId().toStdString();
    auto selectedItem = 0;
    for (std::size_t index = 0; index < displayedSources.size(); ++index)
    {
        const auto& source = displayedSources[index];
        sourceSelector.addItem(juce::String::fromUTF8(source.displayName.c_str()),
                               static_cast<int>(index) + 1);
        if (source.sourceId == selectedSource)
            selectedItem = static_cast<int>(index) + 1;
    }

    if (selectedItem == 0)
    {
        selectedItem = 1;
        processor.setSelectedSourceId(
            juce::String::fromUTF8(displayedSources.front().sourceId.c_str()));
    }
    sourceSelector.setSelectedId(selectedItem, juce::dontSendNotification);
    sourceSelector.setEnabled(true);
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
