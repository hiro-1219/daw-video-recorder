// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include <juce_graphics/juce_graphics.h>
#include <juce_core/juce_core.h>

#include <memory>
#include <string>

namespace dawvr
{
class MFVideoDecoder final
{
public:
    MFVideoDecoder();
    ~MFVideoDecoder();

    bool open(const juce::File& videoFile);
    void close();

    [[nodiscard]] bool isOpen() const noexcept;
    [[nodiscard]] double getDurationSeconds() const noexcept;
    [[nodiscard]] int getWidth() const noexcept;
    [[nodiscard]] int getHeight() const noexcept;
    [[nodiscard]] const juce::File& getCurrentFile() const noexcept;

    // Decodes a video frame closest to the specified time (in seconds).
    // Returns a valid juce::Image on success, or an invalid image on failure.
    juce::Image decodeFrameAt(double timeInSeconds);

private:
    struct Impl;
    std::unique_ptr<Impl> pimpl;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(MFVideoDecoder)
};
} // namespace dawvr
