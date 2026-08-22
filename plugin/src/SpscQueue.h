// SPDX-License-Identifier: AGPL-3.0-only
#pragma once

#include <array>
#include <atomic>
#include <cstddef>
#include <type_traits>

namespace dawvr
{
template <typename Value, std::size_t capacity>
class SpscQueue final
{
    static_assert(capacity >= 2U);
    static_assert((capacity & (capacity - 1U)) == 0U, "capacity must be a power of two");
    static_assert(std::is_trivially_copyable_v<Value>);

public:
    [[nodiscard]] bool push(const Value& value) noexcept
    {
        const auto currentWrite = writeIndex.load(std::memory_order_relaxed);
        const auto nextWrite = increment(currentWrite);
        if (nextWrite == readIndex.load(std::memory_order_acquire))
            return false;

        values[currentWrite] = value;
        writeIndex.store(nextWrite, std::memory_order_release);
        return true;
    }

    [[nodiscard]] bool pop(Value& value) noexcept
    {
        const auto currentRead = readIndex.load(std::memory_order_relaxed);
        if (currentRead == writeIndex.load(std::memory_order_acquire))
            return false;

        value = values[currentRead];
        readIndex.store(increment(currentRead), std::memory_order_release);
        return true;
    }

private:
    [[nodiscard]] static constexpr std::size_t increment(std::size_t index) noexcept
    {
        return (index + 1U) & (capacity - 1U);
    }

    alignas(64) std::array<Value, capacity> values{};
    alignas(64) std::atomic<std::size_t> writeIndex{};
    alignas(64) std::atomic<std::size_t> readIndex{};
};
} // namespace dawvr

