#!/bin/bash

flatpak install --noninteractive -y --user flathub org.freedesktop.Platform//24.08
flatpak install --noninteractive -y --user flathub org.freedesktop.Sdk//24.08 
flatpak install --noninteractive -y --user flathub org.freedesktop.Sdk.Extension.rust-stable//24.08
flatpak install --noninteractive -y --user flathub org.freedesktop.Sdk.Extension.llvm18//24.08
