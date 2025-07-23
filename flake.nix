{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix }:
    let
      forAllSystems = nixpkgs.lib.genAttrs nixpkgs.lib.systems.flakeExposed;
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ fenix.overlays.default ];
          };
          toolchain = fenix.packages.${system}.stable.withComponents [
            "rustc"
            "cargo"
            "rust-std"
            "clippy-preview"
            "rust-analyzer-preview"
            "rust-src"
          ];
          nightlyToolchain = fenix.packages.${system}.latest.withComponents [
            "rustfmt-preview"
          ];
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              cargo-nextest
              foundry
              nightlyToolchain
              openssl
              pkg-config
              toolchain
            ];

            RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
          };
        }
      );
    };
}
