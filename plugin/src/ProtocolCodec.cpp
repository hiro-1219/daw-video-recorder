// SPDX-License-Identifier: AGPL-3.0-only
#include "ProtocolCodec.h"

#include <bit>
#include <cstring>
#include <limits>

namespace dawvr::protocol
{
namespace
{
constexpr std::uint32_t protocolVersion = 1;

enum WireType : std::uint8_t
{
    varint = 0,
    fixed64 = 1,
    lengthDelimited = 2,
    fixed32 = 5
};

void appendVarint(Bytes& destination, std::uint64_t value)
{
    while (value >= 0x80U)
    {
        destination.push_back(static_cast<std::uint8_t>(value) | 0x80U);
        value >>= 7U;
    }
    destination.push_back(static_cast<std::uint8_t>(value));
}

void appendKey(Bytes& destination, std::uint32_t field, WireType wire)
{
    appendVarint(destination, (static_cast<std::uint64_t>(field) << 3U) | wire);
}

void appendUnsigned(Bytes& destination, std::uint32_t field, std::uint64_t value)
{
    if (value == 0U)
        return;
    appendKey(destination, field, varint);
    appendVarint(destination, value);
}

void appendSigned(Bytes& destination, std::uint32_t field, std::int64_t value)
{
    if (value == 0)
        return;
    appendKey(destination, field, varint);
    appendVarint(destination, static_cast<std::uint64_t>(value));
}

void appendBool(Bytes& destination, std::uint32_t field, bool value)
{
    if (value)
        appendUnsigned(destination, field, 1U);
}

void appendString(Bytes& destination, std::uint32_t field, std::string_view value)
{
    if (value.empty())
        return;
    appendKey(destination, field, lengthDelimited);
    appendVarint(destination, value.size());
    destination.insert(destination.end(), value.begin(), value.end());
}

void appendMessage(Bytes& destination, std::uint32_t field, const Bytes& value)
{
    appendKey(destination, field, lengthDelimited);
    appendVarint(destination, value.size());
    destination.insert(destination.end(), value.begin(), value.end());
}

void appendDouble(Bytes& destination, std::uint32_t field, double value)
{
    appendKey(destination, field, fixed64);
    auto bits = std::bit_cast<std::uint64_t>(value);
    for (auto index = 0U; index < sizeof(bits); ++index)
    {
        destination.push_back(static_cast<std::uint8_t>(bits & 0xffU));
        bits >>= 8U;
    }
}

Bytes makeEnvelope(std::uint64_t requestId, std::uint32_t payloadField, const Bytes& payload)
{
    Bytes envelope;
    envelope.reserve(payload.size() + 24U);
    appendUnsigned(envelope, 1, protocolVersion);
    appendUnsigned(envelope, 2, requestId);
    appendMessage(envelope, payloadField, payload);
    return envelope;
}

Bytes makeTransportSnapshot(std::string_view runtimeInstanceId, const TransportSnapshot& snapshot)
{
    Bytes message;
    message.reserve(128U);
    appendString(message, 1, runtimeInstanceId);
    appendUnsigned(message, 2, snapshot.sequenceNumber);
    appendSigned(message, 3, snapshot.callbackMonotonicTimeNs);
    if ((snapshot.flags & TransportFlags::hostTimeValid) != 0U)
        appendSigned(message, 4, snapshot.hostTimeNs);
    if ((snapshot.flags & TransportFlags::projectSampleValid) != 0U)
        appendSigned(message, 5, snapshot.projectSamplePosition);
    appendDouble(message, 6, snapshot.sampleRate);
    appendBool(message, 7, (snapshot.flags & TransportFlags::playing) != 0U);
    appendBool(message, 8, (snapshot.flags & TransportFlags::recording) != 0U);
    appendBool(message, 9, (snapshot.flags & TransportFlags::looping) != 0U);
    appendBool(message, 10, (snapshot.flags & TransportFlags::offlineRendering) != 0U);
    if ((snapshot.flags & TransportFlags::tempoValid) != 0U)
        appendDouble(message, 11, snapshot.tempo);
    if ((snapshot.flags & TransportFlags::timeSignatureValid) != 0U)
    {
        appendSigned(message, 12, snapshot.timeSignatureNumerator);
        appendSigned(message, 13, snapshot.timeSignatureDenominator);
    }
    if ((snapshot.flags & TransportFlags::ppqValid) != 0U)
        appendDouble(message, 14, snapshot.ppqPosition);
    return message;
}

class Reader final
{
public:
    explicit Reader(std::span<const std::uint8_t> input) : remaining(input) {}

    bool next(std::uint32_t& field, std::uint8_t& wire)
    {
        std::uint64_t key{};
        if (!readVarint(key) || key == 0U)
            return false;
        field = static_cast<std::uint32_t>(key >> 3U);
        wire = static_cast<std::uint8_t>(key & 0x07U);
        return true;
    }

    bool readVarint(std::uint64_t& value)
    {
        value = 0U;
        for (auto shift = 0U; shift < 64U && !remaining.empty(); shift += 7U)
        {
            const auto byte = remaining.front();
            remaining = remaining.subspan(1);
            value |= static_cast<std::uint64_t>(byte & 0x7fU) << shift;
            if ((byte & 0x80U) == 0U)
                return true;
        }
        return false;
    }

    bool readBytes(std::span<const std::uint8_t>& value)
    {
        std::uint64_t length{};
        if (!readVarint(length) || length > remaining.size())
            return false;
        value = remaining.first(static_cast<std::size_t>(length));
        remaining = remaining.subspan(static_cast<std::size_t>(length));
        return true;
    }

    bool skip(std::uint8_t wire)
    {
        std::uint64_t ignored{};
        std::span<const std::uint8_t> bytes;
        switch (wire)
        {
            case varint: return readVarint(ignored);
            case fixed64:
                if (remaining.size() < 8U) return false;
                remaining = remaining.subspan(8U);
                return true;
            case lengthDelimited: return readBytes(bytes);
            case fixed32:
                if (remaining.size() < 4U) return false;
                remaining = remaining.subspan(4U);
                return true;
            default: return false;
        }
    }

private:
    std::span<const std::uint8_t> remaining;
};

std::string toString(std::span<const std::uint8_t> bytes)
{
    return { reinterpret_cast<const char*>(bytes.data()), bytes.size() };
}

bool decodeNested(std::uint32_t payloadField,
                  std::span<const std::uint8_t> payload,
                  Response& response)
{
    switch (payloadField)
    {
        case 30: response.kind = ResponseKind::helloAck; break;
        case 31: response.kind = ResponseKind::instanceRegistered; break;
        case 33: response.kind = ResponseKind::sessionStateChanged; break;
        case 35: response.kind = ResponseKind::error; break;
        case 36: response.kind = ResponseKind::heartbeatAck; break;
        case 37: response.kind = ResponseKind::transportAccepted; break;
        default: response.kind = ResponseKind::unknown; break;
    }

    Reader reader(payload);
    std::uint32_t field{};
    std::uint8_t wire{};
    while (reader.next(field, wire))
    {
        std::uint64_t number{};
        std::span<const std::uint8_t> bytes;
        const auto isString = wire == lengthDelimited && reader.readBytes(bytes);
        const auto isNumber = wire == varint && reader.readVarint(number);
        if (!isString && !isNumber)
            return false;

        if (response.kind == ResponseKind::helloAck)
        {
            if (field == 1U && isNumber) response.selectedVersion = static_cast<std::uint32_t>(number);
            if (field == 2U && isString) response.serviceVersion = toString(bytes);
        }
        else if (response.kind == ResponseKind::instanceRegistered)
        {
            if (field == 1U && isString) response.persistentInstanceId = toString(bytes);
            if (field == 2U && isString) response.runtimeInstanceId = toString(bytes);
        }
        else if (response.kind == ResponseKind::sessionStateChanged)
        {
            if (field == 1U && isString) response.runtimeInstanceId = toString(bytes);
            if (field == 2U && isNumber) response.sessionState = static_cast<std::int32_t>(number);
            if (field == 3U && isString) response.message = toString(bytes);
        }
        else if (response.kind == ResponseKind::error)
        {
            if (field == 1U && isString) response.errorCode = toString(bytes);
            if (field == 2U && isString) response.message = toString(bytes);
            if (field == 3U && isString) response.runtimeInstanceId = toString(bytes);
        }
        else if (response.kind == ResponseKind::heartbeatAck && field == 1U && isNumber)
        {
            response.sequenceNumber = number;
        }
        else if (response.kind == ResponseKind::transportAccepted)
        {
            if (field == 1U && isString) response.runtimeInstanceId = toString(bytes);
            if (field == 2U && isNumber) response.sequenceNumber = number;
        }
    }
    return response.kind != ResponseKind::unknown;
}
} // namespace

Bytes makeHello(std::uint64_t requestId)
{
    Bytes hello;
    appendUnsigned(hello, 1, protocolVersion);
    appendUnsigned(hello, 2, protocolVersion);
    appendString(hello, 3, "DAW Video Recorder VST3");
    return makeEnvelope(requestId, 10, hello);
}

Bytes makeRegisterInstance(std::uint64_t requestId,
                           std::string_view persistentInstanceId,
                           std::string_view instanceLabel)
{
    Bytes message;
    appendString(message, 1, persistentInstanceId);
    appendString(message, 2, instanceLabel);
    return makeEnvelope(requestId, 11, message);
}

Bytes makeUnregisterInstance(std::uint64_t requestId, std::string_view runtimeInstanceId)
{
    Bytes message;
    appendString(message, 1, runtimeInstanceId);
    return makeEnvelope(requestId, 12, message);
}

Bytes makeSetProjectRoot(std::uint64_t requestId,
                         std::string_view runtimeInstanceId,
                         std::string_view projectRoot)
{
    Bytes message;
    appendString(message, 1, runtimeInstanceId);
    appendString(message, 2, projectRoot);
    return makeEnvelope(requestId, 15, message);
}

Bytes makeSetArmed(std::uint64_t requestId, std::string_view runtimeInstanceId, bool armed)
{
    Bytes message;
    appendString(message, 1, runtimeInstanceId);
    appendBool(message, 2, armed);
    return makeEnvelope(requestId, 16, message);
}

Bytes makeTransportBatch(std::uint64_t requestId,
                         std::string_view runtimeInstanceId,
                         std::span<const TransportSnapshot> snapshots)
{
    Bytes batch;
    for (const auto& snapshot : snapshots)
        appendMessage(batch, 1, makeTransportSnapshot(runtimeInstanceId, snapshot));
    return makeEnvelope(requestId, 17, batch);
}

bool decodeResponse(std::span<const std::uint8_t> bytes, Response& response)
{
    response = {};
    Reader reader(bytes);
    std::uint32_t field{};
    std::uint8_t wire{};
    std::uint32_t payloadField{};
    std::span<const std::uint8_t> payload;

    while (reader.next(field, wire))
    {
        if (field == 2U && wire == varint)
        {
            if (!reader.readVarint(response.requestId)) return false;
        }
        else if (field >= 30U && wire == lengthDelimited)
        {
            payloadField = field;
            if (!reader.readBytes(payload)) return false;
        }
        else if (!reader.skip(wire))
        {
            return false;
        }
    }

    return payloadField != 0U && decodeNested(payloadField, payload, response);
}
} // namespace dawvr::protocol
