cmake_minimum_required(VERSION 3.25)

project(DAWVideoRecorder VERSION 0.1.0 LANGUAGES C CXX)

option(DAWVR_BUILD_PLUGIN "Build the JUCE VST3 plug-in" ON)
option(DAWVR_BUILD_TEST_TOOLS "Build local integration test tools" ON)
set(DAWVR_JUCE_DIR "" CACHE PATH "Path to a JUCE source checkout")

if(DAWVR_BUILD_PLUGIN)
    if(DAWVR_JUCE_DIR)
        add_subdirectory("${DAWVR_JUCE_DIR}" "${CMAKE_BINARY_DIR}/JUCE")
    elseif(EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/third_party/JUCE/CMakeLists.txt")
        add_subdirectory(third_party/JUCE)
    else()
        message(FATAL_ERROR
            "JUCE was not found. Set DAWVR_JUCE_DIR or place JUCE in third_party/JUCE. "
            "JUCE must be used under terms compatible with this AGPL-3.0-only project.")
    endif()

    add_subdirectory(plugin)
    if(DAWVR_BUILD_TEST_TOOLS)
        add_subdirectory(tools/ipc-smoke)
    endif()
endif()
