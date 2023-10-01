#!/bin/bash

remote_url="git@github.com:mario-eth/soldeer-versions.git"
# remote_url="git@github.com:mario-eth/soldeer-versions.git"
git_name="Soldeer CI"
git_email="ci@soldeer.com"


if [ -z "$1" ]; then
    echo "Please provide a commit message as an argument."
    exit 1
fi

if [ -z "$2" ]; then
    echo "Please provide a key"
    exit 1
fi

rm package-lock.json
rm package.json

# Start ssh-agent and add your key
eval $(ssh-agent)
ssh-add "$2"

cd zipped && \
git init && \
(git remote get-url origin > /dev/null 2>&1 || git remote add origin "$remote_url") && \
git fetch origin && \
git pull origin main && \
git config user.name "$git_name" && \
git config user.email "$git_email" && \
git add . && \
git commit -m "$1" && \
git push -u origin main

# Kill the ssh-agent after use
ssh-agent -k