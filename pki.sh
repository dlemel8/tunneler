#!/bin/bash

set -xe

CA_PRIVATE_KEY_PATH=$1
TARGET_DIR=pki

to_der () {
    type_=$1
    path=$2
    openssl $type_ -in $path \
                   -out $path.der \
                   -outform DER
}


rm -fr $TARGET_DIR
mkdir $TARGET_DIR

openssl req -nodes \
            -x509 \
            -days 3650 \
	    -key $CA_PRIVATE_KEY_PATH \
            -out $TARGET_DIR/ca.crt \
            -sha256 \
            -batch \
            -subj "/CN=Tunneler CA" \
            -extensions v3_ca \
	    -config openssl.cnf

to_der x509 $TARGET_DIR/ca.crt

openssl req -nodes \
            -newkey rsa:2048 \
            -keyout $TARGET_DIR/server.key \
            -out $TARGET_DIR/server.req \
            -sha256 \
            -batch \
            -subj "/CN=Tunneler Server"

to_der rsa $TARGET_DIR/server.key

openssl req -nodes \
            -newkey rsa:2048 \
            -keyout $TARGET_DIR/client.key \
            -out $TARGET_DIR/client.req \
            -sha256 \
            -batch \
            -subj "/CN=Tunneler Client"

to_der rsa $TARGET_DIR/client.key

openssl x509 -req \
             -in $TARGET_DIR/server.req \
             -out $TARGET_DIR/server.crt \
             -CA $TARGET_DIR/ca.crt \
             -CAkey $CA_PRIVATE_KEY_PATH \
             -sha256 \
             -days 365 \
             -set_serial 1 \
             -extensions v3_server \
	     -extfile openssl.cnf


to_der x509 $TARGET_DIR/server.crt

openssl x509 -req \
             -in $TARGET_DIR/client.req \
             -out $TARGET_DIR/client.crt \
             -CA $TARGET_DIR/ca.crt \
             -CAkey $CA_PRIVATE_KEY_PATH \
             -sha256 \
             -days 365 \
             -set_serial 2 \
             -extensions v3_client \
	     -extfile openssl.cnf 

to_der x509 $TARGET_DIR/client.crt

