# SPDX-License-Identifier: Apache-2.0

include(FetchContent)
FetchContent_Declare(
  googletest
  GIT_REPOSITORY https://github.com/google/googletest.git
  GIT_TAG v1.17.0
  FIND_PACKAGE_ARGS NAMES GTest
)
FetchContent_MakeAvailable(googletest)

add_library(libhwlens_test)
target_include_directories(libhwlens_test PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(libhwlens_test libhwlens hwlens_client hwlens_daemon hwlens_extra hwlens_core GTest::gtest_main)

add_executable(hwlens_test cpu_names.cpp extra.cpp json.cpp proc_arrange.cpp server.cpp tools.cpp)
target_link_libraries(hwlens_test libhwlens_test)

include(GoogleTest)
gtest_discover_tests(hwlens_test)
