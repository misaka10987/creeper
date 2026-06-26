{
  description = "devshell";

  inputs = {
    system.url = "path:/etc/nixos";
    nixpkgs.follows = "system/nixpkgs";

    # nixpkgs.url = "git+https://mirrors.nju.edu.cn/git/nixpkgs.git?ref=nixos-unstable&shallow=1";
  };

  outputs =
    { self, nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };
    in
    {
      devShells.${system}.default =
        let
          fhs = pkgs.buildFHSEnv {
            name = "fhs";

            targetPkgs =
              pkgs: with pkgs; [
                bash

                # OpenGL / Vulkan
                libGL
                glfw3-minecraft
                libglvnd
                vulkan-loader

                # X11
                libx11
                libxxf86vm
                libxext
                libxcursor
                libxrandr
                libxtst

                # Audio
                libpulseaudio
                alsa-lib
                openal

                # Wayland / GTK
                wayland
                gtk3
                glib

                libxft
                fontconfig
                freetype
              ];
          };
        in
        pkgs.mkShell {
          packages = [
            fhs
          ];
        };
    };
}
