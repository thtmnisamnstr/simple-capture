set -eu
cd "$(dirname "$0")/../assets/fonts"
EMOJI=8998f5dd683424a73e2314a8c1f1e359c19e8742
GF=cc5661d43cd4f9aad5567033a8e15ca28dc35c86
fetch() {
  if [ ! -f "$1" ]; then curl -sfL -o "$1" "$2"; fi
  echo "$3  $1" | shasum -a 256 -c -
}
gf() { fetch "$2" "https://raw.githubusercontent.com/google/fonts/$GF/$1" "$3"; }
fetch Noto-COLRv1.ttf "https://raw.githubusercontent.com/googlefonts/noto-emoji/$EMOJI/fonts/Noto-COLRv1.ttf" 0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2
fetch LICENSE-NotoColorEmoji-OFL.txt "https://raw.githubusercontent.com/googlefonts/noto-emoji/$EMOJI/fonts/LICENSE" 6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2
gf "ofl/oswald/Oswald%5Bwght%5D.ttf" "Oswald[wght].ttf" 5b38c246e255a12f5712d640d56bcced0472466fc68983d2d0410ec0457c2817
gf "ofl/oswald/OFL.txt" "LICENSE-Oswald-OFL.txt" 0fd731a904b729a4e02eaf5e8ebd06783edd9abe400e8882760160230675b652
gf "ofl/caveat/Caveat%5Bwght%5D.ttf" "Caveat[wght].ttf" 0bdb6b660482d31531b3945849fba5916b3ef8695da7024a9e6b9ee3c4157988
gf "ofl/caveat/OFL.txt" "LICENSE-Caveat-OFL.txt" 1f9d81d094273d82f3898a1ee8b598a717d050ecbf5ff7bede105b704880157b
gf "ofl/inter/Inter%5Bopsz%2Cwght%5D.ttf" "Inter[opsz,wght].ttf" 29160a80ff49ddcab2c97711247e08b1fab27a484a329ce8b813d820dc559031
gf "ofl/inter/OFL.txt" "LICENSE-Inter-OFL.txt" 5b9321a4298cfeb6b34354164a1c3afc3db114569984c502b9b35d988fd58c57
gf "ofl/inter/Inter-Italic%5Bopsz%2Cwght%5D.ttf" "Inter-Italic[opsz,wght].ttf" acd98e64795781b2058f07b18475e0ecee2a0fe2b42a49e2f9e37d0d6bf66ce6
gf "ofl/montserrat/Montserrat%5Bwght%5D.ttf" "Montserrat[wght].ttf" 0f7b311b2f3279e4eef9b2f968bcdbab6e28f4daeb1f049f4f278a902bcd82f7
gf "ofl/montserrat/OFL.txt" "LICENSE-Montserrat-OFL.txt" 8b7141c03fa4f8d44e6345d5d4931709290f0f67875e452e95ac1fd3a027802e
gf "ofl/montserrat/Montserrat-Italic%5Bwght%5D.ttf" "Montserrat-Italic[wght].ttf" 51607f316bc020e59f03cbf51543eecffbea501c0b31d73e5b82927c5cca442c
gf "ofl/bebasneue/BebasNeue-Regular.ttf" "BebasNeue-Regular.ttf" 08e4623805102d819f58601e46e345648846075e363b2ceb23313c2d1c83ec73
gf "ofl/bebasneue/OFL.txt" "LICENSE-BebasNeue-OFL.txt" 72082f6cb4d04be2ecf7cc7d9e1e7d73787f0af8a5a278a47cade70c16b78341
gf "ofl/playfairdisplay/PlayfairDisplay%5Bwght%5D.ttf" "PlayfairDisplay[wght].ttf" c40f2293766a503bc70cce9e512ef844a4ccb7cbcde792fe2ea31d191917d8d6
gf "ofl/playfairdisplay/OFL.txt" "LICENSE-PlayfairDisplay-OFL.txt" 566be814f8e96e93dfa16101331557eb6b5467e9e03f627c0910fe93ca12300e
gf "ofl/playfairdisplay/PlayfairDisplay-Italic%5Bwght%5D.ttf" "PlayfairDisplay-Italic[wght].ttf" a5e26dc5e2e77fb2803a0bf02fd4f81ee136ec8dea863ccdb0c59a263b21378b
gf "ofl/courierprime/CourierPrime-Regular.ttf" "CourierPrime-Regular.ttf" 72f793376f8e2841656bf21d77a5de010f2929bd6956a22ee848ad0c7eb978af
gf "ofl/courierprime/OFL.txt" "LICENSE-CourierPrime-OFL.txt" 9a755af092b494944c99f471be6fddd19b006a448fefdc4717e4ee0aa09a97b0
gf "ofl/courierprime/CourierPrime-Bold.ttf" "CourierPrime-Bold.ttf" ff1f38786c849d1c41fa8e447960abdb2bd75fdfb0cfcdeb524fad65a5af3638
gf "ofl/courierprime/CourierPrime-Italic.ttf" "CourierPrime-Italic.ttf" f1b9a5829789f7e56432a9f3bc7665ef4531dbba1c112639e48bef39621a006b
gf "ofl/courierprime/CourierPrime-BoldItalic.ttf" "CourierPrime-BoldItalic.ttf" 3355273f3ea6d6362658f9564f4e83d59e156cf1a182e3c1592b5c5943627dcc
gf "apache/specialelite/SpecialElite-Regular.ttf" "SpecialElite-Regular.ttf" a776fcb4ceb8bdf03e2967688ebdad42680de5b91a7e62c17e718ae212d14bc4
gf "apache/specialelite/LICENSE.txt" "LICENSE-SpecialElite-Apache-2.0.txt" cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30
gf "ofl/pacifico/Pacifico-Regular.ttf" "Pacifico-Regular.ttf" 5b6c0d5334a7bf77dea52b975c5a0c408878c0f7115ed5b6fb151f634b7bf701
gf "ofl/pacifico/OFL.txt" "LICENSE-Pacifico-OFL.txt" a47e5daeda73568969395c656823102678f2eefb0d7d7ecb47aac4cc17e42204
gf "ofl/tiltneon/TiltNeon%5BXROT%2CYROT%5D.ttf" "TiltNeon[XROT,YROT].ttf" c22333dfd1cd14c2a612b36ea1c50ea13e5e9244181e5129a63c944a165823a0
gf "ofl/tiltneon/OFL.txt" "LICENSE-TiltNeon-OFL.txt" 97941703cd5fb1d5e86a062c9463ed12a613dbfbab9961c333699bdb2d277321
gf "ofl/baloo2/Baloo2%5Bwght%5D.ttf" "Baloo2[wght].ttf" d47a6852548059b1db49a1319d06d499d546c3fa2237cf9eee9c43c8abb025c2
gf "ofl/baloo2/OFL.txt" "LICENSE-Baloo2-OFL.txt" ad09b05dc8bc678c9daf7c4c5f7ef1f55e5726127f4330b2e98e40b9dffcb860
gf "ofl/sedgwickavedisplay/SedgwickAveDisplay-Regular.ttf" "SedgwickAveDisplay-Regular.ttf" 3b9c295a146bafb93c8a12c1dde38a2fe47512777daf1458825e291752469a38
gf "ofl/sedgwickavedisplay/OFL.txt" "LICENSE-SedgwickAveDisplay-OFL.txt" f323edc1e5a6d8fa16120aa15367cdfb15468474cc621f9bda1a2f73c5d08d73
gf "ofl/abrilfatface/AbrilFatface-Regular.ttf" "AbrilFatface-Regular.ttf" 5971d4a3758a922a9fedc7f6fb825a96341a2e718c45a4b2c9a6b417c8c4dbe9
gf "ofl/abrilfatface/OFL.txt" "LICENSE-AbrilFatface-OFL.txt" aba8997e16b1e3888c6e855ba883c70d96bd4375bff8cb9c7ce0f097200f74b8
