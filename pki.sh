#!/bin/bash

TARGET_DIR_DEFAULT=./pki

set -e

verify_openssl() {
  openssl version > /dev/null || (echo "ERROR: could not find an openssl executable" && exit 1)
}

print_usage() {
  local DESCRIPTION="generate one or more of the following, using OpenSSL and a private key:
  \t * CA certificate
  \t * Server private key and certificate, signed by CA
  \t * Client private key and certificate, signed by CA
  \t generated files are in PEM format. each file has a twin in DER format, with .der prefix.
  "
  local USAGE="${0} [options] ca|server|client (multiple arguments supported)"
  local OPTIONS=(
    "-h, --help \t\t print usage and exit"
    "-k, --private_key \t private key file (e.g. SSH RSA key) that will be used as CA key"
    "-t, --target_dir \t generated files directory (default: ${TARGET_DIR_DEFAULT})"
    "-x, --xtrace \t\t set xtrace flag"
  )

  printf 'Description:\n \t %b\n' "${DESCRIPTION}"
  printf 'Usage:\n \t %b\n' "${USAGE}"
  printf 'Options:\n'
  printf '\t %b\n' "${OPTIONS[@]}"
}

parse_flags() {
  while [[ $# -gt 0 ]]; do
    case $1 in
      -h | --help)
        print_usage && exit 0
        ;;
      -k | --private_key)
        test -f "$2" || (echo "ERROR: please provide an existing CA private key path" && exit 1)
        CA_PRIVATE_KEY_PATH=$2
        shift 2
        ;;
      -t | --target_dir)
        test -d "$2" || (echo "ERROR: please provide an existing target directory path" && exit 1)
        TARGET_DIR=$2
        shift 2
        ;;
      ca)
        GENERATE_CA=true
        shift
        ;;
      server)
        GENERATE_SERVER=true
        shift
        ;;
      client)
        GENERATE_CLIENT=true
        shift
        ;;
      -x | --xtrace)
        set -x
        shift
        ;;
      *)
        echo "ERROR: unknown option ${1}" && print_usage && exit 1
        ;;
    esac
  done

  if [[ -z ${CA_PRIVATE_KEY_PATH} ]]; then
    echo "ERROR: CA private key path must be provided" && print_usage && exit 1
  fi

  if [[ -z ${TARGET_DIR} ]]; then
    TARGET_DIR=${TARGET_DIR_DEFAULT}
  fi
}

verify_openssl
parse_flags "$@"

if [[ -n ${GENERATE_CA} ]]; then
  openssl req -nodes \
              -x509 \
              -days 3650 \
              -key "$CA_PRIVATE_KEY_PATH" \
              -out $TARGET_DIR/ca.crt \
              -sha256 \
              -batch \
              -subj "/CN=Tunneler CA" \
              -extensions v3_ca \
              -config openssl.cnf
fi

if [[ -n ${GENERATE_SERVER} ]]; then
  openssl req -nodes \
              -newkey rsa:2048 \
              -keyout $TARGET_DIR/server.key \
              -out $TARGET_DIR/server.req \
              -sha256 \
              -batch \
              -subj "/CN=Tunneler Server"

  openssl x509 -req \
               -in $TARGET_DIR/server.req \
               -out $TARGET_DIR/server.crt \
               -CA $TARGET_DIR/ca.crt \
               -CAkey "$CA_PRIVATE_KEY_PATH" \
               -sha256 \
               -days 365 \
               -set_serial 1 \
               -extensions v3_server \
         -extfile openssl.cnf
fi

if [[ -n ${GENERATE_CLIENT} ]]; then
  openssl req -nodes \
              -newkey rsa:2048 \
              -keyout $TARGET_DIR/client.key \
              -out $TARGET_DIR/client.req \
              -sha256 \
              -batch \
              -subj "/CN=Tunneler Client"

  openssl x509 -req \
               -in $TARGET_DIR/client.req \
               -out $TARGET_DIR/client.crt \
               -CA $TARGET_DIR/ca.crt \
               -CAkey "$CA_PRIVATE_KEY_PATH" \
               -sha256 \
               -days 365 \
               -set_serial 2 \
               -extensions v3_client \
         -extfile openssl.cnf
fi
