# Generated/maintained with AI; see repository disclaimers.md.
cmake_minimum_required(VERSION 3.31)
set(DPHM_CUDA_TARGETS 89-real 120-real)
if(NOT DEFINED CMAKE_CUDA_ARCHITECTURES)
  set(CMAKE_CUDA_ARCHITECTURES "${DPHM_CUDA_TARGETS}")
endif()
if(NOT CMAKE_CUDA_ARCHITECTURES)
  message(FATAL_ERROR "DPHM supports only 89-real and 120-real; target list is empty")
endif()
foreach(arch IN LISTS CMAKE_CUDA_ARCHITECTURES)
  if(NOT arch IN_LIST DPHM_CUDA_TARGETS)
    message(FATAL_ERROR "DPHM supports only 89-real and 120-real; got '${arch}'")
  endif()
endforeach()
project(dphm_cuda LANGUAGES CXX CUDA)

find_package(CUDAToolkit 13.3 REQUIRED)

# Use only cuCollections headers; keep CCCL/toolkit versions from pixi instead
# of running its parent build (which fetches a separate RAPIDS/CCCL stack).
include(FetchContent)
FetchContent_Declare(cuco
  URL https://codeload.github.com/NVIDIA/cuCollections/tar.gz/8ed532dc0ba3ba664d1d41027269908ddcde04c8
  URL_HASH SHA256=676f95da52d8ac6a6b3f494b33757112755322685d010dfa30779e200b412ec6
  SOURCE_SUBDIR headers-only)
FetchContent_MakeAvailable(cuco)

add_library(dphm_cuda_native SHARED c_api.cpp solver.cu)
target_include_directories(dphm_cuda_native SYSTEM PRIVATE ${cuco_SOURCE_DIR}/include)
target_compile_features(dphm_cuda_native PRIVATE cxx_std_23 cuda_std_23)
target_link_libraries(dphm_cuda_native PRIVATE CUDA::cudart)
target_compile_options(dphm_cuda_native PRIVATE
  $<$<COMPILE_LANGUAGE:CXX>:-Wall;-Wextra;-Wpedantic>
  $<$<COMPILE_LANGUAGE:CUDA>:-lineinfo;--extended-lambda;-Xcompiler=-Wall,-Wextra>)

enable_testing()
add_executable(test_ast_lowering test_ast_lowering.cpp)
target_compile_features(test_ast_lowering PRIVATE cxx_std_23)
target_compile_options(test_ast_lowering PRIVATE -Wall -Wextra -Wpedantic -UNDEBUG)
add_test(NAME ast_lowering COMMAND test_ast_lowering)

add_executable(test_frontier test_frontier.cu)
target_compile_features(test_frontier PRIVATE cuda_std_23)
target_link_libraries(test_frontier PRIVATE CUDA::cudart)
target_compile_options(test_frontier PRIVATE -UNDEBUG -Xcompiler=-Wall,-Wextra)
add_test(NAME frontier COMMAND test_frontier)
