#!/bin/bash

scrot -s -e $'curl -s -F file=@$f -F options=\'{"useOriginalFilename":false}\' http://localhost:8088 | jq -j .url | xclip; rm $f' && notify-send "Upload finished" "Screenshot uploaded successfully"
