// SPDX-License-Identifier: AGPL-3.0-only
#include "PluginEditor.h"

#ifndef DAWVR_GIT_COMMIT_HASH
#define DAWVR_GIT_COMMIT_HASH "dev"
#endif

namespace dawvr
{
PluginEditor::PluginEditor(PluginProcessor& processorToUse)
    : AudioProcessorEditor(processorToUse), processor(processorToUse)
{
    // Header
    titleLabel.setText("DAW Video Recorder [" DAWVR_GIT_COMMIT_HASH "]", juce::dontSendNotification);
    titleLabel.setFont(juce::FontOptions(16.0F, juce::Font::bold));
    titleLabel.setColour(juce::Label::textColourId, juce::Colours::white);
    addAndMakeVisible(titleLabel);

    engineStatusLabel.setFont(juce::FontOptions(12.0F, juce::Font::plain));
    engineStatusLabel.setColour(juce::Label::textColourId, juce::Colour(0xff88aacc));
    engineStatusLabel.setJustificationType(juce::Justification::centredLeft);
    addAndMakeVisible(engineStatusLabel);

    serviceStatusLabel.setColour(juce::Label::textColourId, juce::Colours::orange);
    serviceStatusLabel.setJustificationType(juce::Justification::centredRight);
    addAndMakeVisible(serviceStatusLabel);

    // Left pane: Video Preview
    addAndMakeVisible(previewComponent);

    // Right pane: Controls
    addAndMakeVisible(controlsGroup);

    recordArmButton.setColour(juce::ToggleButton::textColourId, juce::Colours::white);
    armAttachment = std::make_unique<juce::AudioProcessorValueTreeState::ButtonAttachment>(
        processor.parameters(), "recordArm", recordArmButton);
    addAndMakeVisible(recordArmButton);

    previewToggleButton.setToggleState(processor.isPreviewEnabled(), juce::dontSendNotification);
    previewToggleButton.onClick = [this] {
        processor.setPreviewEnabled(previewToggleButton.getToggleState());
    };
    addAndMakeVisible(previewToggleButton);

    sourceLabel.setText("Camera Source", juce::dontSendNotification);
    sourceLabel.setFont(juce::FontOptions(13.0F, juce::Font::bold));
    addAndMakeVisible(sourceLabel);

    sourceSelector.addItem("Searching for cameras...", 1);
    sourceSelector.setSelectedId(1, juce::dontSendNotification);
    sourceSelector.setEnabled(false);
    sourceSelector.onChange = [this] {
        const auto index = sourceSelector.getSelectedId() - 1;
        if (index >= 0 && static_cast<std::size_t>(index) < displayedSources.size())
        {
            processor.setSelectedSourceId(
                juce::String::fromUTF8(
                    displayedSources[static_cast<std::size_t>(index)].sourceId.c_str()));
            processor.setPreviewEnabled(previewToggleButton.getToggleState());
        }
    };
    addAndMakeVisible(sourceSelector);

    projectRootLabel.setText("Project Root Folder", juce::dontSendNotification);
    projectRootLabel.setFont(juce::FontOptions(13.0F, juce::Font::bold));
    addAndMakeVisible(projectRootLabel);

    projectRootEditor.setText(processor.projectRoot(), false);
    projectRootEditor.onFocusLost = [this] { processor.setProjectRoot(projectRootEditor.getText()); };
    addAndMakeVisible(projectRootEditor);

    browseButton.onClick = [this] { chooseProjectRoot(); };
    addAndMakeVisible(browseButton);

    encodingLabel.setText("Encoding Profile", juce::dontSendNotification);
    encodingLabel.setFont(juce::FontOptions(13.0F, juce::Font::bold));
    addAndMakeVisible(encodingLabel);

    encodingSelector.addItem("Efficient H.264 / 4 Mbps", 1);
    encodingSelector.addItem("Balanced H.264 / 8 Mbps", 2);
    encodingSelector.addItem("Quality H.264 / 16 Mbps", 3);
    encodingSelector.setSelectedId(processor.encodingPreset(), juce::dontSendNotification);
    encodingSelector.onChange = [this] {
        processor.setEncodingPreset(encodingSelector.getSelectedId());
    };
    addAndMakeVisible(encodingSelector);

    hardwareEncoderButton.setToggleState(processor.prefersHardwareEncoding(),
                                         juce::dontSendNotification);
    hardwareEncoderButton.onClick = [this] {
        processor.setPrefersHardwareEncoding(hardwareEncoderButton.getToggleState());
    };
    addAndMakeVisible(hardwareEncoderButton);

    syncOffsetLabel.setText("Sync Offset", juce::dontSendNotification);
    syncOffsetLabel.setFont(juce::FontOptions(13.0F, juce::Font::bold));
    addAndMakeVisible(syncOffsetLabel);

    syncOffsetSlider.setRange(-5'000.0, 5'000.0, 1.0);
    syncOffsetSlider.setValue(processor.syncOffsetMilliseconds(), juce::dontSendNotification);
    syncOffsetSlider.setTextValueSuffix(" ms");
    syncOffsetSlider.setSliderStyle(juce::Slider::LinearHorizontal);
    syncOffsetSlider.setTextBoxStyle(juce::Slider::TextBoxRight, false, 80, 22);
    syncOffsetSlider.onValueChange = [this] {
        processor.setSyncOffsetMilliseconds(syncOffsetSlider.getValue());
    };
    addAndMakeVisible(syncOffsetSlider);

    // Footer
    metricsLabel.setColour(juce::Label::textColourId, juce::Colour(0xff9999aa));
    metricsLabel.setFont(juce::FontOptions(12.0F, juce::Font::plain));
    addAndMakeVisible(metricsLabel);

    lastTakeLabel.setColour(juce::Label::textColourId, juce::Colour(0xffbbbbcc));
    lastTakeLabel.setFont(juce::FontOptions(12.0F, juce::Font::plain));
    addAndMakeVisible(lastTakeLabel);

    openTakeFolderButton.setEnabled(false);
    openTakeFolderButton.onClick = [this] { openLatestTakeFolder(); };
    addAndMakeVisible(openTakeFolderButton);

    setResizable(true, true);
    setResizeLimits(680, 460, 1400, 900);
    setSize(880, 540);

    processor.startServiceClient();
    processor.setPreviewEnabled(true);
    processor.requestSourceRefresh();
    startTimerHz(15);
}

PluginEditor::~PluginEditor()
{
    stopTimer();
}

void PluginEditor::paint(juce::Graphics& graphics)
{
    graphics.fillAll(juce::Colour(0xff18181c));

    // Header divider line
    graphics.setColour(juce::Colour(0xff2a2a32));
    graphics.drawHorizontalLine(42, 0.0f, static_cast<float>(getWidth()));

    // Footer divider line
    graphics.drawHorizontalLine(getHeight() - 44, 0.0f, static_cast<float>(getWidth()));
}

void PluginEditor::resized()
{
    auto totalArea = getLocalBounds();

    // Top Header (Height: 42px)
    auto headerArea = totalArea.removeFromTop(42).reduced(12, 4);
    titleLabel.setBounds(headerArea.removeFromLeft(250));
    engineStatusLabel.setBounds(headerArea.removeFromLeft(260));
    serviceStatusLabel.setBounds(headerArea);

    // Bottom Footer (Height: 44px)
    auto footerArea = totalArea.removeFromBottom(44).reduced(12, 6);
    openTakeFolderButton.setBounds(footerArea.removeFromRight(100));
    footerArea.removeFromRight(10);
    metricsLabel.setBounds(footerArea.removeFromRight(260));
    lastTakeLabel.setBounds(footerArea);

    // Center Content (2 Panes)
    auto contentArea = totalArea.reduced(10, 8);

    // Right Control Pane (Fixed width: 310px)
    auto rightPane = contentArea.removeFromRight(310);
    controlsGroup.setBounds(rightPane);
    auto controlsInner = rightPane.reduced(12, 20);

    // Record Arm Button
    recordArmButton.setBounds(controlsInner.removeFromTop(30));
    controlsInner.removeFromTop(4);

    // Preview Toggle Button
    previewToggleButton.setBounds(controlsInner.removeFromTop(24));
    controlsInner.removeFromTop(8);

    // Camera Source
    sourceLabel.setBounds(controlsInner.removeFromTop(18));
    sourceSelector.setBounds(controlsInner.removeFromTop(26));
    controlsInner.removeFromTop(8);

    // Encoding
    encodingLabel.setBounds(controlsInner.removeFromTop(18));
    encodingSelector.setBounds(controlsInner.removeFromTop(26));
    controlsInner.removeFromTop(4);
    hardwareEncoderButton.setBounds(controlsInner.removeFromTop(22));
    controlsInner.removeFromTop(8);

    // Project Root
    projectRootLabel.setBounds(controlsInner.removeFromTop(18));
    auto rootRow = controlsInner.removeFromTop(26);
    browseButton.setBounds(rootRow.removeFromRight(75));
    rootRow.removeFromRight(6);
    projectRootEditor.setBounds(rootRow);
    controlsInner.removeFromTop(8);

    // Sync Offset
    syncOffsetLabel.setBounds(controlsInner.removeFromTop(18));
    syncOffsetSlider.setBounds(controlsInner.removeFromTop(26));

    // Left Pane: Video Preview (Takes remaining area)
    contentArea.removeFromRight(8); // Gap
    previewComponent.setBounds(contentArea);
}

void PluginEditor::timerCallback()
{
    updateSourceSelector();

    const auto connectionState = processor.serviceConnectionState();
    serviceStatusLabel.setText(processor.serviceStatusText(), juce::dontSendNotification);
    const auto colour = connectionState == ServiceConnectionState::connected
                            ? juce::Colour(0xff4caf50)
                            : (connectionState == ServiceConnectionState::error ? juce::Colour(0xffe53935)
                                                                                : juce::Colour(0xffff9800));
    serviceStatusLabel.setColour(juce::Label::textColourId, colour);

    // Engine & DAW status display
    juce::String engineText = "Audio: ";
    if (processor.isAudioProcessingActive())
        engineText += "Active (processing)";
    else
        engineText += "Idle (no audio block)";

    engineText += " | DAW: ";
    if (processor.isDawRecording())
        engineText += "Recording";
    else if (processor.isDawPlaying())
        engineText += "Playing";
    else
        engineText += "Stopped";

    engineStatusLabel.setText(engineText, juce::dontSendNotification);

    metricsLabel.setText(
        "Sent: " + juce::String(processor.sentSnapshotCount())
            + " | Drops: " + juce::String(processor.droppedSnapshotCount()),
        juce::dontSendNotification);

    const auto latestTake = processor.latestFinalizedTake();
    if (latestTake.revision != 0U && latestTake.revision != displayedTakeRevision)
    {
        displayedTakeRevision = latestTake.revision;
        currentVideoPath = latestTake.videoPath;
        lastTakeLabel.setText("Last take: " + juce::File(latestTake.videoPath).getFileName(),
                              juce::dontSendNotification);
        lastTakeLabel.setTooltip(latestTake.videoPath);
        openTakeFolderButton.setEnabled(juce::File(currentVideoPath).existsAsFile());
    }

    // Update preview frame from service client
    previewComponent.setFrameData(processor.latestPreviewFrame());

    // Update preview overlay state
    const auto isArmed = processor.isArmed();
    const auto isRecording = processor.isDawRecording();
    if (isRecording && !wasRecording)
    {
        recordingStartTimeMs = juce::Time::currentTimeMillis();
        wasRecording = true;
    }
    else if (!isRecording && wasRecording)
    {
        wasRecording = false;
    }

    std::int64_t duration = 0;
    if (wasRecording)
    {
        duration = juce::Time::currentTimeMillis() - recordingStartTimeMs;
    }
    previewComponent.updateState(connectionState,
                                 processor.serviceStatusText(),
                                 isArmed,
                                 wasRecording,
                                 duration);
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

void PluginEditor::openLatestTakeFolder()
{
    if (currentVideoPath.isNotEmpty())
    {
        const juce::File videoFile(currentVideoPath);
        if (videoFile.existsAsFile())
            videoFile.revealToUser();
        else if (videoFile.getParentDirectory().exists())
            videoFile.getParentDirectory().startAsProcess();
    }
}
} // namespace dawvr
