cmake_minimum_required(VERSION 3.10)
project(drcom_c_abi_test C)

set(CMAKE_C_STANDARD 99)
set(CMAKE_C_STANDARD_REQUIRED ON)

# Allow the user to point to the directory containing the drcom library.
if(NOT DEFINED DRCOM_LIB_DIR)
    set(DRCOM_LIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../target/release")
endif()

find_path(
    DRCOM_INCLUDE_DIR
    drcom.h
    HINTS "${CMAKE_CURRENT_SOURCE_DIR}/../../include"
    NO_DEFAULT_PATH
)

if(WIN32)
    # The MSVC import library is named drcom.dll.lib; find_library() would
    # append another suffix, so look up the exact file instead.
    find_file(
        DRCOM_LIBRARY
        NAMES drcom.dll.lib drcom.lib
        HINTS "${DRCOM_LIB_DIR}"
        NO_DEFAULT_PATH
    )
else()
    find_library(
        DRCOM_LIBRARY
        NAMES drcom
        HINTS "${DRCOM_LIB_DIR}"
        NO_DEFAULT_PATH
    )
endif()

if(NOT DRCOM_INCLUDE_DIR)
    message(FATAL_ERROR "drcom.h not found.  Is include/ in the project root?")
endif()

if(NOT DRCOM_LIBRARY)
    message(FATAL_ERROR "drcom library not found in ${DRCOM_LIB_DIR}.  Build it first with: cargo build --release")
endif()

message(STATUS "drcom include: ${DRCOM_INCLUDE_DIR}")
message(STATUS "drcom library: ${DRCOM_LIBRARY}")

add_executable(test_abi test_abi.c)
target_include_directories(test_abi PRIVATE ${DRCOM_INCLUDE_DIR})
target_link_libraries(test_abi PRIVATE ${DRCOM_LIBRARY})

if(WIN32)
    target_link_libraries(test_abi PRIVATE ws2_32)
else()
    find_package(Threads REQUIRED)
    target_link_libraries(test_abi PRIVATE Threads::Threads ${CMAKE_DL_LIBS})
endif()

# Copy the test config into the build directory so the executable can find it.
configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/test_config.toml
    ${CMAKE_CURRENT_BINARY_DIR}/test_config.toml
    COPYONLY
)

# On Windows also copy the DLL next to the executable so it loads without PATH tweaks.
if(WIN32 AND EXISTS "${DRCOM_LIB_DIR}/drcom.dll")
    file(COPY ${DRCOM_LIB_DIR}/drcom.dll DESTINATION ${CMAKE_CURRENT_BINARY_DIR})
endif()
