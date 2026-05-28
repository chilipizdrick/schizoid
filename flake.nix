{
  description = "schizoid nix flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    nix-github-actions = {
      url = "github:nix-community/nix-github-actions";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs:
    inputs.flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux"];
      perSystem = {pkgs, ...}: {
        packages = rec {
          schizoid = pkgs.callPackage ./package.nix {};
          default = schizoid;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            pkg-config

            bashInteractive
            sqlx-cli
            sqlite
          ];
        };
      };

      flake.githubActions = inputs.nix-github-actions.lib.mkGithubMatrix {checks = inputs.self.packages;};
    };
}
