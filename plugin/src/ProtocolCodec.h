// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include "TransportSnapshot.h"

#include <cstdint>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace dawvr::protocol
{
inline constexpr std::uint32_t currentProtocolVersion = 2U;
using Bytes = std::vector<std::uint8_t>;

enum class ResponseKind
{
    unknown,
    helloAck,
    instanceRegistered,
    sourceList,
    sessionStateChanged,
    recordingFinalized,
    error,
    heartbeatAck,
    transportAccepted,
    previewFrame
};

struct CameraSource final
{
    std::string sourceId;
    std::string displayName;
    bool available{};

    bool operator==(const CameraSource&) const = default;
};

struct Response final
{
    ResponseKind kind{ ResponseKind::unknown };
    std::uint64_t requestId{};
    std::uint64_t sequenceNumber{};
    std::uint32_t selectedVersion{};
    std::int32_t sessionState{};
    std::string persistentInstanceId;
    std::string runtimeInstanceId;
    std::string serviceVersion;
    std::string errorCode;
    std::string message;
    std::string takeId;
    std::string videoPath;
    std::string metadataPath;
    std::vector<CameraSource> sources;

    // Preview frame fields
    std::uint32_t width{};
    std::uint32_t height{};
    std::string format;
    std::vector<std::uint8_t> frameData;
    std::int64_t timestampNs{};
};

[[nodiscard]] Bytes makeHello(std::uint64_t requestId);
[[nodiscard]] Bytes makeRegisterInstance(std::uint64_t requestId,
                                         std::string_view persistentInstanceId,
                                         std::string_view instanceLabel);
[[nodiscard]] Bytes makeUnregisterInstance(std::uint64_t requestId,
                                           std::string_view runtimeInstanceId);
[[nodiscard]] Bytes makeListSources(std::uint64_t requestId);
[[nodiscard]] Bytes makeSelectSource(std::uint64_t requestId,
                                     std::string_view runtimeInstanceId,
                                     std::string_view sourceId);
[[nodiscard]] Bytes makeSetProjectRoot(std::uint64_t requestId,
                                       std::string_view runtimeInstanceId,
                                       std::string_view projectRoot);
[[nodiscard]] Bytes makeSetArmed(std::uint64_t requestId,
                                std::string_view runtimeInstanceId,
                                bool armed);
[[nodiscard]] Bytes makeConfigureEncodingProfile(std::uint64_t requestId,
                                                 std::string_view runtimeInstanceId,
                                                 std::uint32_t bitRate,
                                                 std::uint32_t gopDurationMs,
                                                 bool preferHardware);
[[nodiscard]] Bytes makeSetSyncOffset(std::uint64_t requestId,
                                      std::string_view runtimeInstanceId,
                                      std::int64_t offsetNs);
[[nodiscard]] Bytes makeRequestPreview(std::uint64_t requestId,
                                       std::string_view runtimeInstanceId,
                                       std::uint32_t width,
                                       std::uint32_t height,
                                       std::uint32_t maxFps);
[[nodiscard]] Bytes makeStopPreview(std::uint64_t requestId,
                                    std::string_view runtimeInstanceId);
[[nodiscard]] Bytes makeTransportBatch(std::uint64_t requestId,
                                       std::string_view runtimeInstanceId,
                                       std::span<const TransportSnapshot> snapshots);
[[nodiscard]] bool decodeResponse(std::span<const std::uint8_t> bytes, Response& response);
} // namespace dawvr::protocol
