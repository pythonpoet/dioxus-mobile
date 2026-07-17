{
  inputs = {
    nixpkgs.url          = "github:nixos/nixpkgs/master";
    flake-parts.url      = "github:hercules-ci/flake-parts";
    rust-overlay.url     = "github:oxalica/rust-overlay";
    dioxus-cli.url       = "github:DioxusLabs/dioxus";
    android-nixpkgs.url  = "github:tadfisher/android-nixpkgs";
  };

  outputs = { self, flake-parts, ... } @inputs:
    flake-parts.lib.mkFlake { inherit inputs; } {
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


            sessionExpiry = lib.mkOption {
              type    = lib.types.str;
              default = "3 weeks";
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
                SESSION_EXPIRY          = cfg.sessionExpiry;
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
      perSystem = { self', config, pkgs, lib, system, ... }:
        let
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" ];
            targets    = [ "wasm32-unknown-unknown" ];
          };

          rustBuildInputs =
            (with pkgs; [ openssl libiconv pkg-config ])
            ++ lib.optionals pkgs.stdenv.isLinux (with pkgs; [
              glib gtk3 libsoup_3 webkitgtk_4_1 xdotool
            ])
            ++ lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [
              SystemConfiguration IOKit Carbon WebKit Security Cocoa
            ]);

          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          cargoLock  = builtins.fromTOML (builtins.readFile ./Cargo.lock);
          pname     = cargoToml.package.name;
          version   = cargoToml.package.version;
          rev       = toString (self.shortRev or self.dirtyShortRev or self.lastModified or "unknown");

          vendoredDeps = pkgs.rustPlatform.importCargoLock {
            lockFile = ./Cargo.lock;
          };

          wasmBindgenVersion = (lib.findFirst
            (p: p.name == "wasm-bindgen")
            (throw "wasm-bindgen not found in Cargo.lock")
            cargoLock.package
          ).version;

          wasmBindgenHashes = {
            "0.2.121" = {
              hash      = "sha256-ZOMgFNOcGkO66Jz/Z83eoIu+DIzo3Z/vq6Z5g6BDY/w=";
              cargoHash = "sha256-DPdCDPTAPBrbqLUqnCwQu1dePs9lGg85JCJOCIr9qjU=";
            };
          };

          wasmBindgenCli = pkgs.rustPlatform.buildRustPackage rec {
            pname   = "wasm-bindgen-cli";
            version = wasmBindgenVersion;

            src = pkgs.fetchCrate {
              inherit pname version;
              hash = (wasmBindgenHashes.${version}
                or (throw "No hash for wasm-bindgen-cli@${version} — add it to wasmBindgenHashes")).hash;
            };

            cargoHash = (wasmBindgenHashes.${version}
              or (throw "No cargoHash for wasm-bindgen-cli@${version} — add it to wasmBindgenHashes")).cargoHash;

            buildInputs = lib.optionals pkgs.stdenv.isDarwin (
              with pkgs.darwin.apple_sdk.frameworks; [ Security ]
            );

            doCheck = false;
          };
          # Wrap wasm-opt so it exits 0 without running — dx invokes it as a
          # subprocess and there is no official --no-wasm-opt flag yet.
          # Remove this wrapper once the binaryen/dioxus-cli version mismatch
          # is resolved upstream.
          wasmOptStub = pkgs.writeShellScriptBin "wasm-opt" ''
            echo "wasm-opt: stubbed out in Nix build (SIGABRT workaround)" >&2
            # Pass-through: copy input to output so the bundle isn't broken.
            # dx calls: wasm-opt [flags...] <input> -o <output>
            input=""
            output=""
            args=("$@")
            for (( i=0; i<''${#args[@]}; i++ )); do
              case "''${args[$i]}" in
                -o) output="''${args[$((i+1))]}"; i=$((i+1)) ;;
                -*) ;;
                *)  [[ -z "$input" ]] && input="''${args[$i]}" ;;
              esac
            done
            if [[ -n "$input" && -n "$output" && "$input" != "$output" ]]; then
              cp "$input" "$output"
            fi
          '';
        in
        {
          _module.args.pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };

          formatter = pkgs.nixfmt-rfc-style;

          packages.default = pkgs.stdenv.mkDerivation {
            inherit pname;
            version = "${version}-${rev}";
            src     = ./.;

            nativeBuildInputs = [
              pkgs.esbuild
              wasmOptStub
              pkgs.binaryen
              inputs.dioxus-cli.packages.${system}.default
              rustToolchain
              pkgs.rustPlatform.bindgenHook
              wasmBindgenCli
            ] ++ rustBuildInputs;

            buildInputs = rustBuildInputs;

            configurePhase = ''
              runHook preConfigure
              export HOME=$(mktemp -d)
              export CARGO_HOME=$(mktemp -d)
              export SQLX_OFFLINE=true

              mkdir -p .cargo
              cat >> .cargo/config.toml << 'EOF'
              [source.crates-io]
              replace-with = "vendored-sources"

              [source.vendored-sources]
              directory = "${vendoredDeps}"
              EOF
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
              cp -r target/dx/${pname}/release/web $out/bin/web

              mkdir -p $out/share/${pname}
              cp -r src/server/migrations $out/share/${pname}/migrations
              runHook postInstall
            '';

            meta.mainProgram = "server";
          };

          devShells.default = pkgs.mkShell {
            name        = "dioxus-dev";
            buildInputs = rustBuildInputs;
            nativeBuildInputs = with pkgs; [
              esbuild
              binaryen
              rustToolchain
              sqlx-cli
              inputs.dioxus-cli.packages.${system}.default
              wasmBindgenCli
              just
            ];
            shellHook = ''
              export RUST_SRC_PATH="${rustToolchain}/lib/rustlib/src/rust/library"
              echo ""
              echo "Dioxus dev shell — web"
              echo "  dx serve                           start dev server"
              echo "  dx build --release --platform web  production build"
              echo "  just                               list available recipes"
              echo ""
              # Force `cargo` and `rustc` to come from the Nix Rust toolchain
                # and ensure rustup knows about them
                export CARGO_HOME="${builtins.toString ./.cargo}"
                mkdir -p "$CARGO_HOME"
                cat > "$CARGO_HOME/config.toml" <<- 'EOF'
                  [target.aarch64-linux-android]
                  linker = "${pkgs.stdenv.cc.targetPrefix}cc"
                EOF

                # Use the Nix-provided rustup shim (if any) or override PATH
                # Run `dx` with PATH pointing to the Nix rust toolchain first
                echo "Rust toolchain: $(rustc --version)"
                echo "Default target: $(rustc -vV | grep host)"
              zsh
            '';
          };

          devShells.android =
            let
              ndkVersion = "26.1.10909125";

              androidSdk = inputs.android-nixpkgs.sdk.${system} (p: with p; [
                cmdline-tools-latest
                build-tools-34-0-0
                platform-tools
                platforms-android-34
                ndk-26-1-10909125
              ]);

              rustAndroidToolchain = pkgs.rust-bin.stable.latest.default.override {
                extensions = [ "rust-src" "rust-analyzer" "clippy" ];
                targets = [
                  "wasm32-unknown-unknown"
                  "aarch64-linux-android"
                  "armv7-linux-androideabi"
                  "x86_64-linux-android"
                  "i686-linux-android"
                ];
              };
            in
            pkgs.mkShell {
              name = "dioxus-android";

              packages = with pkgs; [
                androidSdk
                jdk17
                rustAndroidToolchain
                inputs.dioxus-cli.packages.${system}.default
                just
                qrencode
                openssl
                openssl.dev
                pkg-config
              ];

              shellHook = ''
                export ANDROID_SDK_ROOT="${androidSdk}/share/android-sdk"
                export ANDROID_HOME="$ANDROID_SDK_ROOT"
                export NDK_HOME="$ANDROID_SDK_ROOT/ndk/${ndkVersion}"
                export ANDROID_NDK_HOME="$NDK_HOME"
                export JAVA_HOME="${pkgs.jdk17}"
                export RUST_SRC_PATH="${rustAndroidToolchain}/lib/rustlib/src/rust/library"
                export PATH="$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$ANDROID_SDK_ROOT/platform-tools:$PATH"
                LOCAL_IP=$(ip route get 1 2>/dev/null | awk '{print $7; exit}')
                echo ""
                echo "Android dev shell — physical device"
                echo "  USB:      plug in, accept prompt → adb devices"
                echo "  Wireless: Developer options → Wireless debugging → Pair device with pairing code"
                echo "            adb pair <phone-ip>:<pair-port>   (enter the 6-digit code)"
                echo "            adb connect <phone-ip>:<main-port>"
                echo ""
                echo "  Your machine IP (for reference):"
                [ -n "$LOCAL_IP" ] && qrencode -t ANSIUTF8 "$LOCAL_IP" || echo "  (could not detect local IP)"
                echo "  $LOCAL_IP"
                echo ""
                zsh
              '';
            };
        };
    };
}

# ── Android physical device pairing ────────────────────────────────────────────
#
# USB (simplest):
#   1. On device: Settings → Developer options → enable USB debugging
#   2. Plug in USB cable
#   3. Accept the "Allow USB debugging?" prompt on the device
#   4. adb devices          (should list your device as "device", not "unauthorized")
#
# Wireless (Android 11+):
#   1. On device: Settings → Developer options → Wireless debugging → Pair device with pairing code
#   2. Note the IP:port and 6-digit code shown on screen
#   3. adb pair <ip>:<port>          (enter the 6-digit code when prompted)
#   4. adb connect <ip>:<port2>      (use the main wireless debugging port, not the pairing port)
#   5. adb devices
#
# Useful commands:
#   adb devices -l                   list connected devices with model info
#   adb -s <serial> logcat           stream logs from a specific device
#   adb reverse tcp:8080 tcp:8080    forward device requests to localhost (dev server)
