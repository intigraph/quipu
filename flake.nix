{
  description = "quipu: AI-native knowledge graph with strict ontology enforcement";

  inputs.nixpkgs.url = "https://nixos.org/channels/nixos-unstable/nixexprs.tar.xz";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in
    {
      packages = forAll (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.quipu;
        quipu = pkgs.rustPlatform.buildRustPackage {
          pname = "quipu";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # `bundled-onnx` downloads the ONNX runtime in build.rs, which the
          # sandbox forbids. Build `full` and point ort at nixpkgs' runtime.
          buildNoDefaultFeatures = true;
          buildFeatures = [ "full" ];

          nativeBuildInputs = [
            pkgs.makeWrapper
            pkgs.pkg-config
          ];
          # Link needs -lsqlite3 (libs in `out`) and headers/.pc (in `dev`).
          # A bare `pkgs.sqlite` picks only `dev`, so take both outputs.
          buildInputs = [
            pkgs.oniguruma
            pkgs.sqlite.out
            pkgs.sqlite.dev
          ];
          # tokenizers -> onig_sys: link nixpkgs' oniguruma, do not vendor it.
          RUSTONIG_SYSTEM_LIBONIG = true;
          # `server` pulls tikv-jemalloc-sys. The cargo setup hook exports a
          # sentinel JEMALLOC_OVERRIDE that does not exist; point it at nixpkgs'.
          JEMALLOC_OVERRIDE = "${pkgs.jemalloc}/lib/libjemalloc${pkgs.stdenv.hostPlatform.extensions.sharedLibrary}";

          # The test suite is large and parts of it need network or services.
          doCheck = false;

          postInstall = ''
            for bin in $out/bin/*; do
              wrapProgram "$bin" --set-default ORT_DYLIB_PATH \
                ${pkgs.onnxruntime}/lib/libonnxruntime${pkgs.stdenv.hostPlatform.extensions.sharedLibrary}
            done
          '';

          meta = {
            description = "AI-native knowledge graph with strict ontology enforcement";
            homepage = "https://intigraph.github.io/quipu/";
            license = pkgs.lib.licenses.asl20;
            mainProgram = "quipu";
          };
        };
      });
    };
}
