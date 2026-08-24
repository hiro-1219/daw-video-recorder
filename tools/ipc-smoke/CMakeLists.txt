juce_add_console_app(DAWVideoRecorderIpcSmoke
    PRODUCT_NAME "DAW Video Recorder IPC Smoke Test")

target_sources(DAWVideoRecorderIpcSmoke PRIVATE
    main.cpp
    ../../plugin/src/MFVideoDecoder.cpp
    ../../plugin/src/MFVideoDecoder.h
    ../../plugin/src/ProtocolCodec.cpp
    ../../plugin/src/ProtocolCodec.h
    ../../plugin/src/RecordedPlaybackWorker.cpp
    ../../plugin/src/RecordedPlaybackWorker.h
    ../../plugin/src/ServiceClient.cpp
    ../../plugin/src/ServiceClient.h
    ../../plugin/src/SpscQueue.h
    ../../plugin/src/TransportSnapshot.h
    ../../plugin/src/VideoPlaybackPolicy.h)

target_include_directories(DAWVideoRecorderIpcSmoke PRIVATE ../../plugin/src)
target_compile_features(DAWVideoRecorderIpcSmoke PRIVATE cxx_std_20)
target_compile_definitions(DAWVideoRecorderIpcSmoke PRIVATE JUCE_USE_CURL=0)

if(MSVC)
    target_compile_options(DAWVideoRecorderIpcSmoke PRIVATE /wd4324)
endif()

target_link_libraries(DAWVideoRecorderIpcSmoke PRIVATE
    juce::juce_core
    juce::juce_graphics
    mfplat
    mfreadwrite
    mfuuid
    propsys
    juce::juce_recommended_config_flags
    juce::juce_recommended_warning_flags)
