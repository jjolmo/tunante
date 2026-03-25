cmake_minimum_required(VERSION 3.10)
project(viogsf C CXX)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_C_STANDARD 11)

# VBA-M GBA emulator sources
set(VBAM_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../viogsf/vbam")

# psflib (reuse from lazygsf, rename symbols to avoid collision)
set(PSFLIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../lazygsf-rs/psflib")

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

find_package(ZLIB REQUIRED)
target_link_libraries(viogsf PRIVATE ZLIB::ZLIB)

# Suppress warnings from VBA-M code
target_compile_options(viogsf PRIVATE -w)

install(TARGETS viogsf ARCHIVE DESTINATION lib)
