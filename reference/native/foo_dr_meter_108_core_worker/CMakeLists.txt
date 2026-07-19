cmake_minimum_required(VERSION 3.25)

cmake_policy(SET CMP0091 NEW)

project(foo_dr_meter_108_core_worker LANGUAGES CXX ASM_MASM)

if(NOT WIN32 OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
  message(FATAL_ERROR "The isolated core worker supports Windows x64 only")
endif()

add_executable(foo_dr_meter_108_core_worker worker.cpp fp_control.asm)
set_target_properties(
  foo_dr_meter_108_core_worker
  PROPERTIES MSVC_RUNTIME_LIBRARY "MultiThreaded"
)
target_compile_features(foo_dr_meter_108_core_worker PRIVATE cxx_std_20)
target_compile_definitions(
  foo_dr_meter_108_core_worker
  PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX
)
target_compile_options(
  foo_dr_meter_108_core_worker
  PRIVATE
    "$<$<COMPILE_LANGUAGE:CXX>:/W4;/WX;/permissive-;/EHsc;/guard:cf;/sdl;/utf-8>"
)
target_link_options(
  foo_dr_meter_108_core_worker
  PRIVATE /DYNAMICBASE /HIGHENTROPYVA /NXCOMPAT /CETCOMPAT
)
target_link_libraries(foo_dr_meter_108_core_worker PRIVATE bcrypt advapi32)

add_executable(foo_dr_meter_108_mini_json_test mini_json_test.cpp)
set_target_properties(
  foo_dr_meter_108_mini_json_test
  PROPERTIES MSVC_RUNTIME_LIBRARY "MultiThreaded"
)
target_compile_features(foo_dr_meter_108_mini_json_test PRIVATE cxx_std_20)
target_compile_options(
  foo_dr_meter_108_mini_json_test
  PRIVATE /W4 /WX /permissive- /EHsc /guard:cf /sdl /utf-8
)
target_link_options(
  foo_dr_meter_108_mini_json_test
  PRIVATE /DYNAMICBASE /HIGHENTROPYVA /NXCOMPAT /CETCOMPAT
)
enable_testing()
add_test(NAME foo_dr_meter_108_mini_json COMMAND foo_dr_meter_108_mini_json_test)

add_library(foo_dr_meter_108_fail_fast_shared SHARED fail_fast_shared.cpp)
set_target_properties(
  foo_dr_meter_108_fail_fast_shared
  PROPERTIES
    OUTPUT_NAME shared
    PREFIX ""
    LINK_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/fail_fast_shared.def"
    MSVC_RUNTIME_LIBRARY "MultiThreaded"
)
target_compile_features(foo_dr_meter_108_fail_fast_shared PRIVATE cxx_std_20)
target_compile_definitions(
  foo_dr_meter_108_fail_fast_shared
  PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX
)
target_compile_options(
  foo_dr_meter_108_fail_fast_shared
  PRIVATE /W4 /WX /permissive- /GS- /sdl
)
target_link_options(
  foo_dr_meter_108_fail_fast_shared
  PRIVATE
    "/DEF:${CMAKE_CURRENT_SOURCE_DIR}/fail_fast_shared.def"
    /NOENTRY
    /NODEFAULTLIB
    /DYNAMICBASE
    /HIGHENTROPYVA
    /NXCOMPAT
    /CETCOMPAT
)
target_link_libraries(foo_dr_meter_108_fail_fast_shared PRIVATE kernel32)
