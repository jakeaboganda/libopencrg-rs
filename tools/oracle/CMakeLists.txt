cmake_minimum_required(VERSION 3.16)
project(crg_oracle C)

set(CRG ${CMAKE_CURRENT_SOURCE_DIR}/OpenCRG/c-api/baselib)
file(GLOB CRG_SOURCES ${CRG}/src/*.c)

add_executable(crg-oracle oracle.c ${CRG_SOURCES})
target_include_directories(crg-oracle PRIVATE ${CRG}/inc)
set_target_properties(crg-oracle PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
# Rust never fuses multiply-adds; keep the C side from doing it either.
target_compile_options(crg-oracle PRIVATE -O2 -ffp-contract=off)
if(UNIX AND NOT APPLE)
    target_link_libraries(crg-oracle m)
endif()
