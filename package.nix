{rustPlatform, ...}:
rustPlatform.buildRustPackage {
  pname = "schizoid";
  version = "0.1.0";

  src = ./.;
  cargoLock = {
    lockFile = ./Cargo.lock;
    outputHashes = {
      "mcping-0.2.0" = "sha256-DzefFMeUF8l9tj+zrOEDnwOaTydMX2MBsnLIVuGQtm4=";
    };
  };
}
