// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include <cstdint>
#include <type_traits>

namespace dawvr
{
enum TransportFlags : std::uint32_t
{
    playing = 1U << 0U,
    recording = 1U << 1U,
    looping = 1U << 2U,
    offlineRendering = 1U << 3U,
    projectSampleValid = 1U << 4U,
    hostTimeValid = 1U << 5U,
    tempoValid = 1U << 6U,
    timeSignatureValid = 1U << 7U,
    ppqValid = 1U << 8U
};

struct TransportSnapshot final
{
    std::uint64_t sequenceNumber{};
    std::int64_t callbackMonotonicTimeNs{};
    std::int64_t hostTimeNs{};
    std::int64_t projectSamplePosition{};
    double sampleRate{};
    double tempo{};
    double ppqPosition{};
    std::int32_t timeSignatureNumerator{};
    std::int32_t timeSignatureDenominator{};
    std::uint32_t flags{};
};

static_assert(std::is_trivially_copyable_v<TransportSnapshot>);
} // namespace dawvr
