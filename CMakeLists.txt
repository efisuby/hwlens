# SPDX-License-Identifier: Apache-2.0
#
# CMake configuration for hwlens
#

cmake_minimum_required(VERSION 3.25)

# Disable in-source builds
if("${CMAKE_CURRENT_SOURCE_DIR}" STREQUAL "${CMAKE_CURRENT_BINARY_DIR}")
  message(FATAL_ERROR "In-source builds are not allowed")
endif()

project("hwlens"
  VERSION 0.1.0
  DESCRIPTION "Terminal system monitor: hardware sensors, GPU, disks, network and processes"
  HOMEPAGE_URL "https://github.com/efisuby/hwlens"
  LANGUAGES CXX
)

include(CheckIPOSupported)
include(CMakeDependentOption)

# Make our Find<Package>.cmake files available

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_COLOR_DIAGNOSTICS ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

option(HWLENS_STATIC "Link hwlens statically" OFF)
option(HWLENS_GPU "Enable GPU support" ON)
cmake_dependent_option(HWLENS_RSMI_STATIC "Link statically to ROCm SMI" OFF "HWLENS_GPU" OFF)

# Enable LTO in release builds by default
if(CMAKE_BUILD_TYPE MATCHES "Rel(ease|WithDebInfo)")
  option(HWLENS_LTO "Enable LTO" ON)
else()
  option(HWLENS_LTO "Enable LTO" OFF)
endif()

if(HWLENS_STATIC)
  # Set this before calling find_package
  set(CMAKE_FIND_LIBRARY_SUFFIXES ".a")
endif()

add_executable(hwlens src/main.cpp)
add_library(libhwlens OBJECT
  src/hwlens.cpp
  src/hwlens_cli.cpp
  src/hwlens_config.cpp
  src/hwlens_draw.cpp
  src/hwlens_input.cpp
  src/hwlens_log.cpp
  src/hwlens_menu.cpp
  src/hwlens_shared.cpp
  src/hwlens_theme.cpp
  src/hwlens_tools.cpp
)

target_link_libraries(hwlens libhwlens)

if(HWLENS_LTO)
  check_ipo_supported()
  set_target_properties(hwlens PROPERTIES INTERPROCEDURAL_OPTIMIZATION ON)
  set_target_properties(libhwlens PROPERTIES INTERPROCEDURAL_OPTIMIZATION ON)
endif()

if(NOT LINUX)
  message(FATAL_ERROR "${CMAKE_SYSTEM_NAME} is not supported, hwlens runs on Linux only")
endif()
target_sources(libhwlens PRIVATE src/linux/hwlens_collect.cpp)
if(HWLENS_GPU)
  add_subdirectory(src/linux/intel_gpu_top)
endif()

# Generate build info
execute_process(
  COMMAND "git" "rev-parse" "--short" "HEAD"
  WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
  OUTPUT_VARIABLE GIT_COMMIT
  OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
set(CONFIGURE_COMMAND
    "cmake -DHWLENS_STATIC=${HWLENS_STATIC} -DHWLENS_GPU=${HWLENS_GPU}"
)
get_filename_component(CXX_COMPILER_BASENAME "${CMAKE_CXX_COMPILER}" NAME)
set(COMPILER "${CXX_COMPILER_BASENAME}")
set(COMPILER_VERSION "${CMAKE_CXX_COMPILER_VERSION}")
configure_file(${CMAKE_CURRENT_SOURCE_DIR}/src/config.h.in ${CMAKE_CURRENT_BINARY_DIR}/config.h @ONLY IMMEDIATE)
set(CMAKE_INCLUDE_CURRENT_DIR ON)

include(CheckCXXSourceCompiles)
include(CheckCXXSymbolExists)

check_cxx_source_compiles(
  "
  #include <version>
  #if __cpp_lib_optional < 202110L
  #error \"Missing __cpp_lib_optional < 202110L\"
  #endif
  int main() { return 0; }
  "
  HAS_CXX_OPTIONAL_MONADS
)
if(NOT HAS_CXX_OPTIONAL_MONADS)
  message(FATAL_ERROR "The compiler doesn't support std::optional::and_then")
endif()
check_cxx_symbol_exists("__cpp_lib_expected" "version" HAS_CXX_EXPECTED)
if(NOT HAS_CXX_EXPECTED)
  message(FATAL_ERROR "The compiler doesn't support std::expected")
endif()
check_cxx_symbol_exists("__cpp_lib_ranges" "version" HAS_CXX_RANGES)
if(NOT HAS_CXX_RANGES)
  message(FATAL_ERROR "The compiler doesn't support std::ranges")
endif()
check_cxx_symbol_exists("__cpp_lib_ranges_to_container" "version" HAS_CXX_RANGES_TO_CONTAINER)
if(NOT HAS_CXX_RANGES_TO_CONTAINER)
  message(FATAL_ERROR "The compiler doesn't support std::ranges::to")
endif()
check_cxx_symbol_exists("__cpp_lib_string_contains" "version" HAS_CXX_STRING_CONTAINS)
if(NOT HAS_CXX_STRING_CONTAINS)
  message(FATAL_ERROR "The compiler doesn't support std::string::contains")
endif()

target_compile_options(libhwlens PUBLIC -Wall -Wextra -Wpedantic)

include(CheckCXXCompilerFlag)

target_compile_options(libhwlens PUBLIC -fstack-clash-protection)
check_cxx_compiler_flag(-fstack-protector HAS_FSTACK_PROTECTOR)
if(HAS_FSTACK_PROTECTOR)
  target_compile_options(libhwlens PUBLIC -fstack-protector)
endif()
check_cxx_compiler_flag(-fcf-protection HAS_FCF_PROTECTION)
if(HAS_FCF_PROTECTION)
  target_compile_options(libhwlens PUBLIC -fcf-protection)
endif()

target_compile_definitions(libhwlens PUBLIC
  FMT_HEADER_ONLY
  _FILE_OFFSET_BITS=64
  $<$<CONFIG:Debug>:_GLIBCXX_ASSERTIONS _LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_DEBUG>
)

target_include_directories(libhwlens SYSTEM PUBLIC include)

# Enable pthreads
set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)
target_link_libraries(libhwlens Threads::Threads)

# Enable GPU support
if(HWLENS_GPU)
  target_compile_definitions(libhwlens PUBLIC GPU_SUPPORT)

  if(HWLENS_RSMI_STATIC)
    # ROCm doesn't properly add it's folders to the module path if `CMAKE_MODULE_PATH` is already
    # set
    # We could also manually append ROCm's path here
    set(_CMAKE_MODULE_PATH CMAKE_MODULE_PATH)
    unset(CMAKE_MODULE_PATH)

    # NOTE: This might be problematic in the future if other sub projects depend on this or if
    # hwlens starts producing libraries
    # Build a static ROCm library
    set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)

    add_subdirectory(lib/rocm_smi_lib EXCLUDE_FROM_ALL)

    add_library(ROCm INTERFACE)
    # Export ROCm's properties to a target
    target_compile_definitions(ROCm INTERFACE RSMI_STATIC)
    target_include_directories(ROCm INTERFACE lib/rocm_smi_lib/include)
    target_link_libraries(ROCm INTERFACE rocm_smi64)

    set(CMAKE_MODULE_PATH _CMAKE_MODULE_PATH)

    target_link_libraries(libhwlens ROCm)
  endif()
  if(NOT HWLENS_STATIC)
    target_link_libraries(libhwlens ${CMAKE_DL_LIBS})
  endif()
endif()

if(HWLENS_STATIC)
  target_compile_definitions(libhwlens PUBLIC STATIC_BUILD)
  target_link_options(libhwlens PUBLIC -static LINKER:--fatal-warnings)
endif()

# Check if lowdown is installed
find_program(LOWDOWN_EXECUTABLE lowdown)

if(LOWDOWN_EXECUTABLE)
  # Custom target to compile Markdown to man page using lowdown
  add_custom_command(
    OUTPUT hwlens.1
    COMMAND lowdown -s -T man -o hwlens.1 ${CMAKE_CURRENT_SOURCE_DIR}/manpage.md
    DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/manpage.md
    VERBATIM
  )
  add_custom_target(generate_manpage ALL DEPENDS ${CMAKE_CURRENT_BINARY_DIR}/hwlens.1)
  # Install the man page
  install(FILES "${CMAKE_CURRENT_BINARY_DIR}/hwlens.1" DESTINATION "share/man/man1")
else()
  message(STATUS "Command 'lowdown' not found: skipping generating man page hwlens.1")
endif()

install(TARGETS hwlens RUNTIME)
install(FILES "hwlens.desktop" DESTINATION "share/applications")
install(FILES "Img/icon.svg" DESTINATION "share/icons/hicolor/scalable/apps" RENAME "hwlens.svg")
install(DIRECTORY "themes" DESTINATION "share/hwlens")

include(CTest)
if(BUILD_TESTING)
  add_subdirectory(tests)
endif()
