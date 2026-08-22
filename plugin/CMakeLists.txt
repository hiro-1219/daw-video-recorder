juce_add_plugin(DAWVideoRecorderPlugin
    COMPANY_NAME "DAW Video Recorder"
    BUNDLE_ID "org.daw-video-recorder.plugin"
    PLUGIN_MANUFACTURER_CODE Dvrc
    PLUGIN_CODE Dvrc
    FORMATS VST3
    PRODUCT_NAME "DAW Video Recorder"
    DESCRIPTION "Camera recording synchronized to a DAW transport"
    IS_SYNTH FALSE
    NEEDS_MIDI_INPUT FALSE
    NEEDS_MIDI_OUTPUT FALSE
    IS_MIDI_EFFECT FALSE
    EDITOR_WANTS_KEYBOARD_FOCUS FALSE
    COPY_PLUGIN_AFTER_BUILD FALSE)

target_sources(DAWVideoRecorderPlugin PRIVATE
    src/PluginEditor.cpp
    src/PluginEditor.h
    src/PluginProcessor.cpp
    src/PluginProcessor.h
    src/ServiceClient.cpp
    src/ServiceClient.h
    src/SpscQueue.h
    src/TransportSnapshot.h)

target_compile_features(DAWVideoRecorderPlugin PRIVATE cxx_std_20)

target_compile_definitions(DAWVideoRecorderPlugin
    PUBLIC
        JUCE_WEB_BROWSER=0
        JUCE_USE_CURL=0
        JUCE_VST3_CAN_REPLACE_VST2=0)

if(MSVC)
    # Cache-line padding in the SPSC queue is intentional.
    target_compile_options(DAWVideoRecorderPlugin PRIVATE /wd4324)
endif()

target_link_libraries(DAWVideoRecorderPlugin
    PRIVATE
        juce::juce_audio_utils
        juce::juce_gui_extra
    PUBLIC
        juce::juce_recommended_config_flags
        juce::juce_recommended_lto_flags
        juce::juce_recommended_warning_flags)
