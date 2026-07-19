#pragma once

#include <charconv>
#include <cstddef>
#include <cstdint>
#include <functional>
#include <map>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <variant>
#include <vector>

namespace mini_json {

struct Limits {
    std::size_t max_input_bytes = 1024 * 1024;
    std::size_t max_depth = 64;
    std::size_t max_string_bytes = 1024 * 1024;
    std::size_t max_container_elements = 100'000;
    std::size_t max_total_values = 200'000;
};

class ParseError final : public std::runtime_error {
  public:
    ParseError(std::size_t offset, std::string message)
        : std::runtime_error(std::move(message)), offset_(offset) {}

    [[nodiscard]] std::size_t offset() const noexcept { return offset_; }

  private:
    std::size_t offset_;
};

class TypeError final : public std::runtime_error {
  public:
    using std::runtime_error::runtime_error;
};

class Value {
  public:
    using Array = std::vector<Value>;
    using Object = std::map<std::string, Value, std::less<>>;

    Value() noexcept = default;
    Value(std::nullptr_t) noexcept : data_(nullptr) {}
    Value(bool value) noexcept : data_(value) {}
    Value(std::uint64_t value) noexcept : data_(value) {}
    Value(std::string value) : data_(std::move(value)) {}
    Value(const char *value) : data_(std::string(value)) {}
    Value(Array value) : data_(std::move(value)) {}
    Value(Object value) : data_(std::move(value)) {}

    [[nodiscard]] bool is_null() const noexcept {
        return std::holds_alternative<std::nullptr_t>(data_);
    }
    [[nodiscard]] bool is_bool() const noexcept {
        return std::holds_alternative<bool>(data_);
    }
    [[nodiscard]] bool is_uint() const noexcept {
        return std::holds_alternative<std::uint64_t>(data_);
    }
    [[nodiscard]] bool is_string() const noexcept {
        return std::holds_alternative<std::string>(data_);
    }
    [[nodiscard]] bool is_array() const noexcept {
        return std::holds_alternative<Array>(data_);
    }
    [[nodiscard]] bool is_object() const noexcept {
        return std::holds_alternative<Object>(data_);
    }

    [[nodiscard]] bool as_bool() const {
        return get<bool>("boolean");
    }
    [[nodiscard]] std::uint64_t as_uint() const {
        return get<std::uint64_t>("unsigned integer");
    }
    [[nodiscard]] const std::string &as_string() const {
        return get<std::string>("string");
    }
    [[nodiscard]] const Array &as_array() const {
        return get<Array>("array");
    }
    [[nodiscard]] Array &as_array() { return get<Array>("array"); }
    [[nodiscard]] const Object &as_object() const {
        return get<Object>("object");
    }
    [[nodiscard]] Object &as_object() { return get<Object>("object"); }

    [[nodiscard]] const Value *find(std::string_view key) const noexcept {
        const auto *object = std::get_if<Object>(&data_);
        if (object == nullptr) {
            return nullptr;
        }
        const auto it = object->find(key);
        return it == object->end() ? nullptr : &it->second;
    }

    [[nodiscard]] Value *find(std::string_view key) noexcept {
        auto *object = std::get_if<Object>(&data_);
        if (object == nullptr) {
            return nullptr;
        }
        const auto it = object->find(key);
        return it == object->end() ? nullptr : &it->second;
    }

    [[nodiscard]] const Value &at(std::string_view key) const {
        const auto &object = as_object();
        const auto it = object.find(key);
        if (it == object.end()) {
            throw TypeError("missing object key: " + std::string(key));
        }
        return it->second;
    }

    [[nodiscard]] Value &at(std::string_view key) {
        auto &object = as_object();
        const auto it = object.find(key);
        if (it == object.end()) {
            throw TypeError("missing object key: " + std::string(key));
        }
        return it->second;
    }

    [[nodiscard]] const Value &at(std::size_t index) const {
        const auto &array = as_array();
        if (index >= array.size()) {
            throw TypeError("array index out of range");
        }
        return array[index];
    }

    [[nodiscard]] Value &at(std::size_t index) {
        auto &array = as_array();
        if (index >= array.size()) {
            throw TypeError("array index out of range");
        }
        return array[index];
    }

  private:
    template <typename T>
    [[nodiscard]] const T &get(const char *expected) const {
        const auto *value = std::get_if<T>(&data_);
        if (value == nullptr) {
            throw TypeError(std::string("JSON value is not a ") + expected);
        }
        return *value;
    }

    template <typename T>
    [[nodiscard]] T &get(const char *expected) {
        auto *value = std::get_if<T>(&data_);
        if (value == nullptr) {
            throw TypeError(std::string("JSON value is not a ") + expected);
        }
        return *value;
    }

    std::variant<std::nullptr_t, bool, std::uint64_t, std::string, Array, Object>
        data_{nullptr};
};

namespace detail {

class Parser final {
  public:
    Parser(std::string_view input, Limits limits)
        : input_(input), limits_(limits) {}

    [[nodiscard]] Value parse() {
        if (input_.size() > limits_.max_input_bytes) {
            fail(0, "JSON input exceeds size limit");
        }
        skip_whitespace();
        Value result = parse_value(0);
        skip_whitespace();
        if (position_ != input_.size()) {
            fail(position_, "trailing bytes after JSON value");
        }
        return result;
    }

  private:
    [[noreturn]] void fail(std::size_t offset, const char *message) const {
        throw ParseError(offset, message);
    }

    [[nodiscard]] bool at_end() const noexcept {
        return position_ == input_.size();
    }

    [[nodiscard]] char peek() const noexcept { return input_[position_]; }

    char take() {
        if (at_end()) {
            fail(position_, "unexpected end of JSON input");
        }
        return input_[position_++];
    }

    bool consume(char expected) noexcept {
        if (!at_end() && peek() == expected) {
            ++position_;
            return true;
        }
        return false;
    }

    void skip_whitespace() noexcept {
        while (!at_end()) {
            const char value = peek();
            if (value != ' ' && value != '\t' && value != '\n' &&
                value != '\r') {
                break;
            }
            ++position_;
        }
    }

    void note_value() {
        if (total_values_ == limits_.max_total_values) {
            fail(position_, "JSON value count exceeds limit");
        }
        ++total_values_;
    }

    [[nodiscard]] Value parse_value(std::size_t depth) {
        note_value();
        if (at_end()) {
            fail(position_, "expected JSON value");
        }

        switch (peek()) {
        case '{':
            if (depth >= limits_.max_depth) {
                fail(position_, "JSON nesting depth exceeds limit");
            }
            return parse_object(depth);
        case '[':
            if (depth >= limits_.max_depth) {
                fail(position_, "JSON nesting depth exceeds limit");
            }
            return parse_array(depth);
        case '"':
            return Value(parse_string());
        case 't':
            parse_literal("true");
            return Value(true);
        case 'f':
            parse_literal("false");
            return Value(false);
        case 'n':
            parse_literal("null");
            return Value(nullptr);
        default:
            if (peek() >= '0' && peek() <= '9') {
                return Value(parse_unsigned_integer());
            }
            fail(position_, "invalid JSON value");
        }
    }

    [[nodiscard]] Value parse_object(std::size_t depth) {
        (void)take();
        skip_whitespace();
        Value::Object object;
        if (consume('}')) {
            return Value(std::move(object));
        }

        for (;;) {
            if (object.size() == limits_.max_container_elements) {
                fail(position_, "JSON object member count exceeds limit");
            }
            if (at_end() || peek() != '"') {
                fail(position_, "expected string object key");
            }
            const std::size_t key_offset = position_;
            std::string key = parse_string();
            skip_whitespace();
            if (!consume(':')) {
                fail(position_, "expected ':' after object key");
            }
            skip_whitespace();
            Value value = parse_value(depth + 1);
            const auto [unused, inserted] =
                object.emplace(std::move(key), std::move(value));
            (void)unused;
            if (!inserted) {
                fail(key_offset, "duplicate object key");
            }
            skip_whitespace();
            if (consume('}')) {
                return Value(std::move(object));
            }
            if (!consume(',')) {
                fail(position_, "expected ',' or '}' in object");
            }
            skip_whitespace();
        }
    }

    [[nodiscard]] Value parse_array(std::size_t depth) {
        (void)take();
        skip_whitespace();
        Value::Array array;
        if (consume(']')) {
            return Value(std::move(array));
        }

        for (;;) {
            if (array.size() == limits_.max_container_elements) {
                fail(position_, "JSON array element count exceeds limit");
            }
            array.push_back(parse_value(depth + 1));
            skip_whitespace();
            if (consume(']')) {
                return Value(std::move(array));
            }
            if (!consume(',')) {
                fail(position_, "expected ',' or ']' in array");
            }
            skip_whitespace();
        }
    }

    static std::uint32_t hex_digit(char value) noexcept {
        if (value >= '0' && value <= '9') {
            return static_cast<std::uint32_t>(value - '0');
        }
        if (value >= 'a' && value <= 'f') {
            return static_cast<std::uint32_t>(value - 'a' + 10);
        }
        if (value >= 'A' && value <= 'F') {
            return static_cast<std::uint32_t>(value - 'A' + 10);
        }
        return 16;
    }

    [[nodiscard]] std::uint32_t parse_hex_quad() {
        std::uint32_t result = 0;
        for (unsigned index = 0; index < 4; ++index) {
            if (at_end()) {
                fail(position_, "truncated Unicode escape");
            }
            const std::uint32_t digit = hex_digit(take());
            if (digit == 16) {
                fail(position_ - 1, "invalid Unicode escape");
            }
            result = (result << 4U) | digit;
        }
        return result;
    }

    void append_byte(std::string &output, char value) {
        if (output.size() == limits_.max_string_bytes) {
            fail(position_, "decoded JSON string exceeds size limit");
        }
        output.push_back(value);
    }

    void append_code_point(std::string &output, std::uint32_t value) {
        char encoded[4];
        std::size_t size = 0;
        if (value <= 0x7FU) {
            encoded[size++] = static_cast<char>(value);
        } else if (value <= 0x7FFU) {
            encoded[size++] = static_cast<char>(0xC0U | (value >> 6U));
            encoded[size++] = static_cast<char>(0x80U | (value & 0x3FU));
        } else if (value <= 0xFFFFU) {
            encoded[size++] = static_cast<char>(0xE0U | (value >> 12U));
            encoded[size++] =
                static_cast<char>(0x80U | ((value >> 6U) & 0x3FU));
            encoded[size++] = static_cast<char>(0x80U | (value & 0x3FU));
        } else {
            encoded[size++] = static_cast<char>(0xF0U | (value >> 18U));
            encoded[size++] =
                static_cast<char>(0x80U | ((value >> 12U) & 0x3FU));
            encoded[size++] =
                static_cast<char>(0x80U | ((value >> 6U) & 0x3FU));
            encoded[size++] = static_cast<char>(0x80U | (value & 0x3FU));
        }
        if (size > limits_.max_string_bytes - output.size()) {
            fail(position_, "decoded JSON string exceeds size limit");
        }
        output.append(encoded, size);
    }

    void append_unicode_escape(std::string &output) {
        std::uint32_t code_point = parse_hex_quad();
        if (code_point >= 0xD800U && code_point <= 0xDBFFU) {
            if (!consume('\\') || !consume('u')) {
                fail(position_, "high surrogate without low surrogate");
            }
            const std::uint32_t low = parse_hex_quad();
            if (low < 0xDC00U || low > 0xDFFFU) {
                fail(position_ - 4, "invalid low surrogate");
            }
            code_point =
                0x10000U + ((code_point - 0xD800U) << 10U) + (low - 0xDC00U);
        } else if (code_point >= 0xDC00U && code_point <= 0xDFFFU) {
            fail(position_ - 4, "unpaired low surrogate");
        }
        append_code_point(output, code_point);
    }

    static std::size_t utf8_sequence_length(unsigned char lead) noexcept {
        if (lead <= 0x7FU) {
            return 1;
        }
        if (lead >= 0xC2U && lead <= 0xDFU) {
            return 2;
        }
        if (lead >= 0xE0U && lead <= 0xEFU) {
            return 3;
        }
        if (lead >= 0xF0U && lead <= 0xF4U) {
            return 4;
        }
        return 0;
    }

    void append_utf8_sequence(std::string &output) {
        const std::size_t start = position_;
        const auto lead = static_cast<unsigned char>(peek());
        const std::size_t length = utf8_sequence_length(lead);
        if (length == 0 || length > input_.size() - position_) {
            fail(position_, "invalid UTF-8 in JSON string");
        }
        for (std::size_t index = 1; index < length; ++index) {
            const auto continuation =
                static_cast<unsigned char>(input_[position_ + index]);
            if ((continuation & 0xC0U) != 0x80U) {
                fail(position_ + index, "invalid UTF-8 continuation byte");
            }
        }

        const auto second = length > 1
                                ? static_cast<unsigned char>(input_[start + 1])
                                : 0U;
        if ((lead == 0xE0U && second < 0xA0U) ||
            (lead == 0xEDU && second > 0x9FU) ||
            (lead == 0xF0U && second < 0x90U) ||
            (lead == 0xF4U && second > 0x8FU)) {
            fail(position_, "invalid UTF-8 scalar value");
        }
        if (length > limits_.max_string_bytes - output.size()) {
            fail(position_, "decoded JSON string exceeds size limit");
        }
        output.append(input_.substr(position_, length));
        position_ += length;
    }

    [[nodiscard]] std::string parse_string() {
        if (take() != '"') {
            fail(position_ - 1, "expected JSON string");
        }
        std::string output;
        while (!at_end()) {
            const auto byte = static_cast<unsigned char>(peek());
            if (byte == '"') {
                ++position_;
                return output;
            }
            if (byte == '\\') {
                ++position_;
                if (at_end()) {
                    fail(position_, "truncated JSON escape");
                }
                switch (take()) {
                case '"':
                    append_byte(output, '"');
                    break;
                case '\\':
                    append_byte(output, '\\');
                    break;
                case '/':
                    append_byte(output, '/');
                    break;
                case 'b':
                    append_byte(output, '\b');
                    break;
                case 'f':
                    append_byte(output, '\f');
                    break;
                case 'n':
                    append_byte(output, '\n');
                    break;
                case 'r':
                    append_byte(output, '\r');
                    break;
                case 't':
                    append_byte(output, '\t');
                    break;
                case 'u':
                    append_unicode_escape(output);
                    break;
                default:
                    fail(position_ - 1, "invalid JSON escape");
                }
            } else if (byte < 0x20U) {
                fail(position_, "unescaped control byte in JSON string");
            } else if (byte < 0x80U) {
                append_byte(output, take());
            } else {
                append_utf8_sequence(output);
            }
        }
        fail(position_, "unterminated JSON string");
    }

    [[nodiscard]] std::uint64_t parse_unsigned_integer() {
        const std::size_t start = position_;
        if (consume('0')) {
            if (!at_end() && peek() >= '0' && peek() <= '9') {
                fail(position_, "leading zero in JSON integer");
            }
        } else {
            while (!at_end() && peek() >= '0' && peek() <= '9') {
                ++position_;
            }
        }
        if (!at_end() &&
            (peek() == '.' || peek() == 'e' || peek() == 'E')) {
            fail(position_, "floating-point JSON numbers are not supported");
        }

        std::uint64_t value = 0;
        const char *first = input_.data() + start;
        const char *last = input_.data() + position_;
        const auto result = std::from_chars(first, last, value, 10);
        if (result.ec == std::errc::result_out_of_range) {
            fail(start, "JSON integer exceeds uint64 range");
        }
        if (result.ec != std::errc{} || result.ptr != last) {
            fail(start, "invalid JSON integer");
        }
        return value;
    }

    void parse_literal(std::string_view literal) {
        if (input_.substr(position_, literal.size()) != literal) {
            fail(position_, "invalid JSON literal");
        }
        position_ += literal.size();
    }

    std::string_view input_;
    Limits limits_;
    std::size_t position_ = 0;
    std::size_t total_values_ = 0;
};

} // namespace detail

[[nodiscard]] inline Value parse(std::string_view input,
                                 Limits limits = Limits{}) {
    return detail::Parser(input, limits).parse();
}

} // namespace mini_json
