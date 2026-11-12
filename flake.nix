{
  # `nix run github:afflom/UORust -- --version` or
  # `nix profile install github:afflom/UORust`: builds the uorust CLI from the
  # locked workspace (Cargo.lock) with the nixpkgs revision pinned below, whose
  # rustc is newer than the workspace's rust-version.
  description = "uorust: project any Cargo workspace into a complete, checked Prism project";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/d4f25a3e562008648d45bcf2d70bcc7422cad84e";

  outputs = { self, nixpkgs }:
    let
      workspace = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package;
      crate = (builtins.fromTOML (builtins.readFile ./crates/uorust/Cargo.toml)).package;
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forEach = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forEach (pkgs: rec {
        uorust = pkgs.rustPlatform.buildRustPackage {
          pname = "uorust";
          inherit (workspace) version;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "--package" "uorust" "--bin" "uorust" ];
          # The tests need the repository's fixtures and the Prism SDK; CI runs them.
          doCheck = false;
          meta = {
            inherit (crate) description;
            homepage = workspace.homepage;
            license = with pkgs.lib.licenses; [ mit asl20 ];
            mainProgram = "uorust";
          };
        };
        default = uorust;
      });
      apps = forEach (pkgs: rec {
        uorust = { type = "app"; program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.uorust}/bin/uorust"; };
        default = uorust;
      });
    };
}
