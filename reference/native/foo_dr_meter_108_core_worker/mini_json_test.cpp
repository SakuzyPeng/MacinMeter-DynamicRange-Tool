#include "mini_json.hpp"

#include <cstdint>
#include <cstdlib>
#include <string>
#include <string_view>

namespace {

void require(bool condition) {
    if (!condition) {
        std::abort();
    }
}

void expect_parse_error(std::string_view input) {
    bool rejected = false;
    try {
        (void)mini_json::parse(input);
    } catch (const mini_json::ParseError &) {
        rejected = true;
    }
    require(rejected);
}

void expect_parse_error(std::string_view input, mini_json::Limits limits) {
    bool rejected = false;
    try {
        (void)mini_json::parse(input, limits);
    } catch (const mini_json::ParseError &) {
        rejected = true;
    }
    require(rejected);
}

} // namespace

int main() {
    const auto value = mini_json::parse(
        R"({"array":[0,18446744073709551615,true,false,null],)"
        R"("escaped":"\"\\\/\b\f\n\r\t",)"
        R"("unicode":"A\u00df\u6771\ud834\udd1e"})");
    require(value.is_object());
    require(value.at("array").at(0).as_uint() == UINT64_C(0));
    require(value.at("array").at(1).as_uint() == UINT64_MAX);
    require(value.at("array").at(2).as_bool());
    require(!value.at("array").at(3).as_bool());
    require(value.at("array").at(4).is_null());
    require(value.at("escaped").as_string() ==
            std::string("\"\\/\b\f\n\r\t"));
    require(value.at("unicode").as_string() ==
            std::string("A\xc3\x9f\xe6\x9d\xb1\xf0\x9d\x84\x9e"));
    require(value.find("missing") == nullptr);

    const std::string_view invalid_inputs[] = {
        "",
        "-1",
        "1.0",
        "1e2",
        "01",
        "18446744073709551616",
        "NaN",
        "Infinity",
        "true false",
        R"({"x":1,"x":2})",
        R"({"x":1,})",
        "[1,]",
        R"("\x")",
        R"("\uD800")",
        R"("\uDC00")",
        std::string_view{"\"\xc0\x80\"", 4},
        std::string_view{"\"\xed\xa0\x80\"", 5},
        std::string_view{"\"\xf4\x90\x80\x80\"", 6},
    };
    for (const std::string_view invalid : invalid_inputs) {
        expect_parse_error(invalid);
    }

    {
        mini_json::Limits limits;
        limits.max_input_bytes = 2;
        expect_parse_error("null", limits);
    }
    {
        mini_json::Limits limits;
        limits.max_depth = 1;
        expect_parse_error("[[]]", limits);
    }
    {
        mini_json::Limits limits;
        limits.max_string_bytes = 3;
        expect_parse_error(R"("\u20acx")", limits);
    }
    {
        mini_json::Limits limits;
        limits.max_container_elements = 1;
        expect_parse_error("[1,2]", limits);
    }
    {
        mini_json::Limits limits;
        limits.max_total_values = 2;
        expect_parse_error("[1,2]", limits);
    }
}
