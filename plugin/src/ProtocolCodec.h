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
using Bytes = std::vector<std::uint8_t>;

enum class ResponseKind
{
    unknown,
    helloAck,
    instanceRegistered,
    sessionStateChanged,
    error,
    heartbeatAck,
    transportAccepted
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
};

[[nodiscard]] Bytes makeHello(std::uint64_t requestId);
[[nodiscard]] Bytes makeRegisterInstance(std::uint64_t requestId,
                                         std::string_view persistentInstanceId,
                                         std::string_view instanceLabel);
[[nodiscard]] Bytes makeUnregisterInstance(std::uint64_t requestId,
                                           std::string_view runtimeInstanceId);
[[nodiscard]] Bytes makeSetProjectRoot(std::uint64_t requestId,
                                       std::string_view runtimeInstanceId,
                                       std::string_view projectRoot);
[[nodiscard]] Bytes makeSetArmed(std::uint64_t requestId,
                                std::string_view runtimeInstanceId,
                                bool armed);
[[nodiscard]] Bytes makeTransportBatch(std::uint64_t requestId,
                                       std::string_view runtimeInstanceId,
                                       std::span<const TransportSnapshot> snapshots);
[[nodiscard]] bool decodeResponse(std::span<const std::uint8_t> bytes, Response& response);
} // namespace dawvr::protocol

