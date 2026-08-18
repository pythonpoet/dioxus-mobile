{
  # ── Binary Cache Configuration ─────────────────────────────────────────────
    nixConfig = {
      extra-substituters = [
        "https://cache.nixos.org"
        "https://nix-community.cachix.org"
        # Add your own cache here, e.g.:
        # "https://your-cache-name.cachix.org"
      ];
      extra-trusted-public-keys = [
        "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
        "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
        # Add your cache public key here, e.g.:
        # "your-cache-name.cachix.org-1:your-public-key-here"
      ];
    };
  inputs = {
    nixpkgs.url          = "github:nixos/nixpkgs/master";
    flake-parts.url      = "github:hercules-ci/flake-parts";
    rust-overlay.url     = "github:oxalica/rust-overlay";
    dioxus-cli.url       = "github:DioxusLabs/dioxus";
    android-nixpkgs = {
      url  = "github:tadfisher/android-nixpkgs/stable";
      inputs.nixpkgs.follows = "nixpkgs";
    }; # Fixed: Added missing semicolon here
    nci = { # Fixed: Changed 'nic' to 'nci' to match inputs.nci.flakeModule
      url = "github:90-008/nix-cargo-integration";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, flake-parts, ... } @inputs:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.nci.flakeModule
      ];

      systems = [ "x86_64-linux" "x86_64-darwin" "aarch64-darwin" "aarch64-linux" ];

      # ── NixOS module ────────────────────────────────────────────────────────
      flake.nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.taalbubbl;
          pkg = cfg.package;
        in {
          options.services.taalbubbl = {
            enable  = lib.mkEnableOption "taalbubbl dioxus server";
            package = lib.mkOption {
              type        = lib.types.package;
              default     = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              description = "The taalbubbl package to run.";
            };

            user  = lib.mkOption { type = lib.types.str;  default = "taalbubbl"; };
            group = lib.mkOption { type = lib.types.str;  default = "taalbubbl"; };

            environmentFile = lib.mkOption {
              type    = lib.types.nullOr lib.types.path;
              default = null;
              example = "/run/secrets/taalbubbl.env";
            };

            port = lib.mkOption { type = lib.types.port; default = 8080; };
            ip   = lib.mkOption { type = lib.types.str;  default = "0.0.0.0"; };

            database = {
              url = lib.mkOption {
                type    = lib.types.str;
                default = "";
              };
              migrationPath = lib.mkOption {
                type    = lib.types.str;
                default = "${pkg}/share/taalbubbl/migrations";
              };
              type = lib.mkOption {
                type    = lib.types.str;
                default = "postgres";
              };
              name = lib.mkOption {
                type = lib.types.str;
                default = "taalbubbl";
                description = "The name of the taalbubbl database.";
              };
              host = lib.mkOption {
                type = lib.types.str;
                default = "/run/postgresql";
                example = "127.0.0.1";
                description = "Hostname or address of the postgresql server. If an absolute path is given here, it will be interpreted as a unix socket path.";
              };
              port = lib.mkOption {
                type = lib.types.port;
                default = 5432;
                description = "Port of the postgresql server.";
              };
              user = lib.mkOption {
                type = lib.types.str;
                default = "taalbubbl";
                description = "The database user for taalbubbl.";
              };
            };
            ml_url = lib.mkOption {
              type = lib.types.str;
              default = "http://localhost:8000";
              description = "url to locate ML api";
            };

            url = lib.mkOption {
              type    = lib.types.str;
              default = "http://localhost:8080";
              example = "https://taalbubbl.example.com";
            };
            use_https = lib.mkOption {
              type    = lib.types.bool;
              default = true;
            };
          };

          config = lib.mkIf cfg.enable {
            security.polkit.extraConfig = ''
              polkit.addRule(function(action, subject) {
                if (
                  action.id == "org.freedesktop.systemd1.manage-units" &&
                  action.lookup("unit") == "taalbubbl.service" &&
                  subject.isInGroup("${cfg.group}")
                ) { return polkit.Result.YES; }
              });
            '';

            users.users.${cfg.user} = {
              isSystemUser = true;
              group        = cfg.group;
            };
            users.groups.${cfg.group} = {};

            systemd.services.taalbubbl = {
              description = "taalbubbl dioxus server";
              wantedBy    = [ "multi-user.target" ];
              after  = [ "network.target" "postgresql.service" ];
              wants  = [ "postgresql.service" ];

              environment = {
                PORT                    = toString cfg.port;
                IP                      = cfg.ip;
                DATABASE_URL            = "postgresql:///${cfg.database.name}?host=${cfg.database.host}";
                DATABASE_MIGRATION_PATH = cfg.database.migrationPath;
                DATABASE_TYPE           = cfg.database.type;
                URL                     = cfg.url;
                USE_HTTPS               = toString cfg.use_https;
                ML_URL                  = cfg.ml_url;
               };

              serviceConfig = {
                User             = cfg.user;
                Group            = cfg.group;
                EnvironmentFile  = cfg.environmentFile;

                WorkingDirectory = "${pkg}/bin/web";
                ExecStart        = "${pkg}/bin/web/server";
                Restart          = "on-failure";
                RestartSec       = "5s";
                NoNewPrivileges  = true;
                PrivateTmp       = true;
                ProtectSystem    = "strict";
              };
            };
          };
        };

      # ── Per-system outputs ──────────────────────────────────────────────────
      perSystem = { config, pkgs, lib, system, ... }:
        let
          # Base build inputs for Linux desktop / macOS framework linking
          rustBuildInputs =
            (with pkgs; [ openssl libiconv pkg-config ])
            ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux (with pkgs; [
              glib gtk3 libsoup_3 webkitgtk_4_1 xdotool
            ])
            ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.apple-sdk ];

          # Mobile Target Arrays
          androidTargets = [
            "aarch64-linux-android"
            "armv7-linux-androideabi"
          ];

          iosTargets = [
            "aarch64-apple-ios"
            "aarch64-apple-ios-sim"
          ];

          # Unified Toolchain sharing targets across Web, Android, and iOS
          unifiedRustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" ];
            targets    = [ "wasm32-unknown-unknown" ] ++ androidTargets ++ iosTargets;
          };

          # Fixed: Defined explicit NDK version variable
          ndkVersion = "26.1.10909125";

          androidSdk = inputs.android-nixpkgs.sdk.${system} (p: with p; [
            cmdline-tools-latest
            build-tools-34-0-0
            platform-tools
            platforms-android-34
            ndk-26-1-10909125
          ]);
        in
        {
          _module.args.pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };

          formatter = pkgs.nixfmt-rfc-style;

          # ── NCI Declarative Configuration ────────────────────────────────────
          nci = {
            crates.default = {
              drvConfig = {
                mkDerivation = {
                  buildInputs = rustBuildInputs;
                  nativeBuildInputs = [
                    pkgs.esbuild
                    pkgs.binaryen
                    inputs.dioxus-cli.packages.${system}.default
                    pkgs.rustPlatform.bindgenHook
                  ];

                  configurePhase = ''
                    runHook preConfigure
                    export HOME=$(mktemp -d)
                    export CARGO_HOME=$(mktemp -d)
                    export SQLX_OFFLINE=true
                    runHook postConfigure
                  '';

                  buildPhase = ''
                    runHook preBuild
                    dx build --release --platform web
                    runHook postBuild
                  '';

                  installPhase = ''
                    runHook preInstall
                    mkdir -p $out/bin
                    cp -r target/dx/taalbubbl/release/web $out/bin/web

                    mkdir -p $out/share/taalbubbl
                    cp -r src/server/migrations $out/share/taalbubbl/migrations
                    runHook postInstall
                  '';
                };
              };
            };
          };

          # ── Mobile DevShells using Unified Toolchain ──────────────────────
          devShells.android = pkgs.mkShell {
            name = "dioxus-android";

            packages = with pkgs; [
              androidSdk
              jdk17
              unifiedRustToolchain
              inputs.dioxus-cli.packages.${system}.default
              just
              qrencode
              openssl
              openssl.dev
              pkg-config
              alsa-lib
            ] ++ rustBuildInputs;

            shellHook = ''
              export ANDROID_SDK_ROOT="${androidSdk}/share/android-sdk"
              export ANDROID_HOME="$ANDROID_SDK_ROOT"
              export NDK_HOME="$ANDROID_SDK_ROOT/ndk/${ndkVersion}"
              export ANDROID_NDK_HOME="$NDK_HOME"
              export JAVA_HOME="${pkgs.jdk17}"
              export RUST_SRC_PATH="${unifiedRustToolchain}/lib/rustlib/src/rust/library"
              export PATH="$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$ANDROID_SDK_ROOT/platform-tools:$PATH"
              LOCAL_IP=$(ip route get 1 2>/dev/null | awk '{print $7; exit}')
            '';
          };

          devShells.ios = pkgs.mkShell {
            name = "dioxus-ios";

            packages = with pkgs; [
              unifiedRustToolchain
              inputs.dioxus-cli.packages.${system}.default
              just
              openssl
              openssl.dev
              pkg-config
            ] ++ rustBuildInputs;

            shellHook = ''
              export RUST_SRC_PATH="${unifiedRustToolchain}/lib/rustlib/src/rust/library"
            '';
          };
        };
    };
}
