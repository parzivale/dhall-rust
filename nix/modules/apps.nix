# `nix run .#update-tests -- missing`
#
# Regenerates missing expected-output files under dhall/tests. Was a loose
# update-tests.sh that assumed dhall and fd were already on PATH; the
# runtimeInputs below are what make that assumption unnecessary.
#
# TODO: this wants breaking up -- the per-folder input/output/process triples
# are begging to be data rather than a wall of shell functions.
{ pkgs }:

{
  update-tests = {
    type = "app";
    program = pkgs.lib.getExe (
      pkgs.writeShellApplication {
        name = "update-tests";
        runtimeInputs = with pkgs; [
          dhall
          fd
          git
        ];
        text = ''
          usage_text=$(cat <<-END
          Usage: update-tests [missing | add]

            missing  Generate any expected-output file that does not exist yet.
            add      Read lines of "<path> <contents>" on stdin and add a test
                     for each, generating its expected output. Eg:
                       normalization/unit/TextShowEmpty Text/show ""
          END
          )

          # `nix run` executes from the store, so find the working tree.
          cd "$(git rev-parse --show-toplevel)" || exit 1

          export DHALL_TEST_VAR="6 * 7"

          # Only our own tests. The dhall-lang suite was listed here too when it
          # was a submodule; it is now pinned by the flake and lives read-only in
          # the store. Its expected outputs come from upstream and are not ours to
          # regenerate.
          TEST_ROOT="dhall/tests"

          parser_input_file() { echo "$1A.dhall"; }
          parser_output_file() { echo "$1B.dhallb"; }
          parser_process() { dhall encode --file "$1"; }

          binary-decode_input_file() { echo "$1A.dhallb"; }
          binary-decode_output_file() { echo "$1B.dhall"; }
          binary-decode_process() { dhall decode --file "$1"; }

          semantic-hash_input_file() { echo "$1A.dhall"; }
          semantic-hash_output_file() { echo "$1B.hash"; }
          semantic-hash_process() { dhall hash --file "$1"; }

          import_input_file() { echo "$1A.dhall"; }
          import_output_file() { echo "$1B.dhall"; }
          import_process() { dhall --file "$1"; }

          type-inference_input_file() { echo "$1A.dhall"; }
          type-inference_output_file() { echo "$1B.dhall"; }
          type-inference_process() { dhall resolve --file "$1" | dhall type; }

          normalization_input_file() { echo "$1A.dhall"; }
          normalization_output_file() { echo "$1B.dhall"; }
          normalization_process() { dhall --file "$1"; }

          alpha-normalization_input_file() { echo "$1A.dhall"; }
          alpha-normalization_output_file() { echo "$1B.dhall"; }
          alpha-normalization_process() { dhall normalize --alpha --file "$1"; }

          tmpfile=$(mktemp -t update-tests.XXXXXX)
          trap 'rm -f "$tmpfile"' EXIT

          generate_output_file() {
              local folder="$1"
              local file="$2"
              local input_file output_file
              input_file="$("''${folder}_input_file" "$file")"
              output_file="$("''${folder}_output_file" "$file")"

              if [ ! -f "$output_file" ]; then
                  echo "$output_file"
                  # Leave the output absent if the tool rejects the input, rather
                  # than writing a half-generated file.
                  if "''${folder}_process" "$input_file" > "$tmpfile"; then
                      mv "$tmpfile" "$output_file"
                  fi
              fi

              # .diag files are CBOR diagnostic notation, for reading parser
              # output by eye. cbor-diag is a ruby gem nixpkgs does not carry, so
              # this is skipped unless you have it; nothing in the test suite
              # reads them.
              if [ -f "$output_file" ] && [ "$folder" = "parser" ] \
                  && [ ! -f "''${file}B.diag" ]; then
                  if command -v cbor2diag.rb > /dev/null; then
                      cbor2diag.rb < "$output_file" > "''${file}B.diag"
                  else
                      echo "  (skipping ''${file}B.diag: no cbor2diag.rb)"
                  fi
              fi
          }

          case "''${1-}" in
          missing)
              echo "Generating missing output files..."
              for folder in parser binary-decode semantic-hash import \
                  type-inference normalization alpha-normalization; do
                  # Most of these only exist in the dhall-lang suite; we carry
                  # local tests for just a couple of them.
                  [ -d "$TEST_ROOT/$folder/success" ] || continue
                  # Not robust to spaces in filenames, but there are none
                  fd 'A\.dhallb?$' "$TEST_ROOT/$folder/success" \
                      | sed 's/A.dhallb\?$//' \
                      | while read -r file; do
                          generate_output_file "$folder" "$file"
                      done
              done
              ;;

          add)
              # Reads lines of a path and file contents, like:
              #   normalization/unit/TextShowEmpty Text/show ""
              while read -r file contents; do
                  folder="$(echo "$file" | cut -d/ -f1)"
                  is_success="$(echo "$file" | cut -d/ -f2)"
                  file="./$TEST_ROOT/$file"
                  mkdir -p "$(dirname "$file")"

                  if [ "$is_success" = "success" ]; then
                      echo "$contents" > "''${file}A.dhall"
                      generate_output_file "$folder" "$file"
                  else
                      echo "$contents" > "''${file}.dhall"
                  fi
              done
              ;;

          *)
              echo "$usage_text"
              ;;
          esac
        '';
      }
    );
  };
}
