{
  rustPlatform,
  pkg-config,
  openssl,
  libopus,
  pkgs,
  ...
}:
rustPlatform.buildRustPackage {
  pname = "schizoid";
  version = "0.1.0";

  src = pkgs.lib.cleanSource ./.;
  cargoLock = {
    lockFile = ./Cargo.lock;
    outputHashes = {
      "mcping-0.2.0" = "sha256-DzefFMeUF8l9tj+zrOEDnwOaTydMX2MBsnLIVuGQtm4=";
    };
  };

  SQLX_OFFLINE = "true";

  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs = [
    openssl
    libopus
  ];
}
