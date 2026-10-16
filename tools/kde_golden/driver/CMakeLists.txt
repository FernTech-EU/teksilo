# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech
#
# Reference-data driver for tools/kde_golden. Built inside the Docker image
# (see ../Dockerfile); it links the distribution's KF6 and Qt 6 libraries and
# calls only their public API.

cmake_minimum_required(VERSION 3.22)
project(kde_golden_driver LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC OFF)

find_package(ECM 6.0 REQUIRED NO_MODULE)
find_package(Qt6 6.6 REQUIRED COMPONENTS Core Gui Widgets)
find_package(KF6ColorScheme REQUIRED)
find_package(KF6GuiAddons REQUIRED)
find_package(KF6Config REQUIRED)

add_executable(kde_golden
    main.cpp
    golden_json.cpp
    math_vectors.cpp
    resolve_vectors.cpp
    size_vectors.cpp
    pixel_vectors.cpp
)
target_link_libraries(kde_golden PRIVATE
    Qt6::Core Qt6::Gui Qt6::Widgets
    KF6::ColorScheme KF6::GuiAddons KF6::ConfigCore KF6::ConfigGui
)
