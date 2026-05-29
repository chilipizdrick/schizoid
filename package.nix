{
  rustPlatform,
  pkg-config,
  openssl,
  libopus,
  ...
}:
rustPlatform.buildRustPackage {
  pname = "schizoid";
  version = "0.1.0";

  src = ./.;
  # cargoLock = {
  #   lockFile = ./Cargo.lock;
  #   outputHashes = {
  #     "mcping-0.2.0" = "sha256-DzefFMeUF8l9tj+zrOEDnwOaTydMX2MBsnLIVuGQtm4=";
  #   };
  # };
  cargoHash = "sha256-qsTKSCdtC6jd9D41pblV0W/AZkwZ4Xq/vD4wptIMtUY=";

  SQLX_OFFLINE = "true";

  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs = [
    openssl
    libopus
  ];
}
