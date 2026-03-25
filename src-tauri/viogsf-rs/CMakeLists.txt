cmake_minimum_required(VERSION 3.10)
project(viogsf C CXX)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_C_STANDARD 11)

# VBA-M GBA emulator sources
set(VBAM_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../viogsf/vbam")

# psflib (reuse from lazygsf, rename symbols to avoid collision)
set(PSFLIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../lazygsf-rs/psflib")

# Bundled zlib (fallback for Windows which doesn't have system zlib)
set(ZLIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../hepsf-rs/zlib")

add_library(viogsf STATIC
    # Wrapper
    ${CMAKE_CURRENT_SOURCE_DIR}/viogsf_wrapper.cpp

    # GBA core
    ${VBAM_DIR}/gba/GBA.cpp
    ${VBAM_DIR}/gba/GBA-arm.cpp
    ${VBAM_DIR}/gba/GBA-thumb.cpp
    ${VBAM_DIR}/gba/Sound.cpp
    ${VBAM_DIR}/gba/bios.cpp

    # APU (audio)
    ${VBAM_DIR}/apu/Blip_Buffer.cpp
    ${VBAM_DIR}/apu/Effects_Buffer.cpp
    ${VBAM_DIR}/apu/Gb_Apu.cpp
    ${VBAM_DIR}/apu/Gb_Oscs.cpp
    ${VBAM_DIR}/apu/Multi_Buffer.cpp

    # psflib (for loading GSF/minigsf containers)
    ${PSFLIB_DIR}/psf2fs.c
    ${PSFLIB_DIR}/psflib.c
)

target_include_directories(viogsf PRIVATE
    ${VBAM_DIR}
    ${VBAM_DIR}/common
    ${VBAM_DIR}/gba
    ${VBAM_DIR}/apu
    ${PSFLIB_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}
)

# Rename psflib symbols to avoid collision with lazygsf-rs and hepsf-rs
target_compile_definitions(viogsf PRIVATE
    -Dpsf_load=viogsf_psf_load
    -Dpsf_load_cancel=viogsf_psf_load_cancel
    -Dpsf2fs_create=viogsf_psf2fs_create
    -Dpsf2fs_delete=viogsf_psf2fs_delete
    -Dpsf2fs_virtual_readfile=viogsf_psf2fs_virtual_readfile
)

# Try system zlib first, fall back to bundled
find_package(ZLIB QUIET)
if(ZLIB_FOUND)
    target_link_libraries(viogsf PRIVATE ZLIB::ZLIB)
else()
    # Use bundled zlib from hepsf-rs
    file(GLOB ZLIB_SOURCES "${ZLIB_DIR}/*.c")
    target_sources(viogsf PRIVATE ${ZLIB_SOURCES})
    target_include_directories(viogsf PRIVATE ${ZLIB_DIR})
endif()

# Suppress warnings from VBA-M code
target_compile_options(viogsf PRIVATE
    $<$<CXX_COMPILER_ID:MSVC>:/w>
    $<$<NOT:$<CXX_COMPILER_ID:MSVC>>:-w>
)

install(TARGETS viogsf ARCHIVE DESTINATION lib)
