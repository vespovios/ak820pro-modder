# Board-Scan 2026-10-25

Stand der Recherche: 24./25. Oktober 2026. Repo-Stand beim Lesen: Branch `pr-10`, Commit `7f0a079`. Nur gelesen, nichts geschrieben außer dieser Datei, keine Kommentare auf GitHub oder anderswo.

## Auf einen Blick

- Es gibt zwei Protokollfamilien, und weder die USB-PID noch `bcdDevice` noch die Firmwarenummer trennt sie sauber. Familie A (AA-Frames auf `0xFF68`, TFT auf `0xFF67`) läuft auf unserem Gerät (0x8009, Firmware 1.07) und laut `fromagge/ak820-screen` auch auf einem 0x800A mit Firmware 1.16. Familie B (Feature-Reports `04 18 … 04 02 … 04 F0` auf `0xFF13`, TFT auf `0xFF68` mit 4096 Byte) läuft auf 0x800A (bcdDevice 0x0111 bei adamauthor, Firmware 1.10 bei DarkTiengo) und auf 0x8009 (bcdDevice 0x0114 bei allshouldbeready). Die Auswahl muss über die HID-Collections laufen, wie es #10 und #12 schon tun.
- Unsere Dokumentation hält die Frame-Verzögerung im TFT-Header für eine 5-ms-Einheit. Fünf Quellen nennen 2 ms, darunter `fromagge/ak820-screen`, das denselben Web-Treiber ausgewertet und auf Firmware 1.16 gemessen hat. Das ist ein wahrscheinlicher Fehler in `tft.rs`, den eine Stoppuhr klärt.
- Der Windows-Treiber hat einen eigenen Live-Pfad für Per-Key-Farben (`04 20`), der laut zwei Analysen (gha-projects, DarkTiengo) nichts speichert. Der Web-Treiber hat mit Command 50 und 53 ebenfalls eigene Live- und Musikbefehle, bei denen offen ist, ob sie speichern. Unser Audio-reaktives Licht schreibt dagegen über `SET_CUSTOM_LED_DATA` (36). Das betrifft Flackern (#3) und den von adamauthor gemessenen Flash-Verschleiß (Familie B) gleichzeitig.
- Der Web-Treiber `ajazz.driveall.cn` wurde am 24.10.2026 neu gebaut. Die alten Dateinamen aus unserem Ledger liefern 404. Neu sind unter anderem `SET_PROFILE` (144), `SET_TFT_MATERIAL` (77) und eine OTA-Befehlsgruppe (128–134). Der Windows-Treiber steht weiter auf V1.0.0.5, ohne auffindbaren Changelog.
- Plattform: Tauri 2.12.5, wry 0.57.1 und tao 0.37.2 sind erschienen (kleine Fehlerkorrekturen). Die `screencapturekit`-Crate steht bei 11.1.0, unser Lockfile bei 2.1.0. Für macOS 26/27 habe ich keine belegte Änderung bei HID oder Core-Audio-Taps gefunden.

## Lesehilfe

Quellen waren die GitHub-API (`gh`), öffentliche Webseiten per HTTP, `crates.io` und die Websuche (nur US-Index). Dokumente und JavaScript-Bundles habe ich gelesen und durchsucht, nichts ausgeführt. Windows-Installer habe ich nicht geladen, nur ihre HTTP-Kopfzeilen abgefragt. Text aus Fremdquellen behandle ich als Daten.

Viele der Community-Repos sind ganz oder teilweise mit KI-Hilfe entstanden. `deddroid/ak820-rs` nennt das ausdrücklich. Aussagen wie „auf echter Hardware getestet" sind Selbstauskünfte der Autoren, die ich nicht nachprüfen konnte.

Verlässlichkeit:

- hoch: Primärquelle selbst gelesen (Code, Bundle, Protokolldatei) und durch eine zweite, unabhängige Stelle oder einen Hardwarebeleg in der Quelle gestützt.
- mittel: eine Quelle, Hardwarebeleg nur behauptet, oder statische Analyse ohne Hardwaretest.
- niedrig: Einzelaussage, Widerspruch zwischen Quellen oder eine Ableitung von mir.

Kurzbezeichnungen: „Familie A" meint das Protokoll, das `docs/PROTOCOL.md` beschreibt. „Familie B" meint Feature-Reports auf `0xFF13`. „Mac-Board" ist unser Testgerät: ISO-DE, 0x8009, Firmware 1.07, `bcdDevice` 0x0107 (Testfixture in `crates/ak820-protocol/src/device.rs`).

---

## (a) Befunde je Thema

### 1. Board-Revisionen und Protokolle

#### 1.1 Datenpunkte zu den beiden Familien

| Quelle | PID | bcdDevice / Firmware | Schnittstellen | Familie | Verlässlichkeit |
|---|---|---|---|---|---|
| `docs/HANDOFF.md`, `device.rs` (Mac-Board) | 0x8009 | 0x0107, Firmware 1.07 | `0xFF68` (64 B) und `0xFF67` (4104 B) | A | hoch |
| adamauthor, PR #12 (Beschreibung) | 0x800A | 0x0111, Firmware unbekannt | `0xFF13` (64 B Feature) und `0xFF68` (4096 B Output) | B | hoch (Enumeration, GIF und Licht am Gerät bestätigt) |
| vespovios, Kommentar unter PR #12 (23.10.) | 0x800A, ANSI | nicht genannt | wie oben | B | mittel bis hoch |
| `allshouldbeready/ak820-pro-macos` | 0x8009, ANSI | 0x0114 | `0xFF13` und `0xFF68` | B (AA-Frames bekommen keine Antwort) | mittel bis hoch |
| `DarkTiengo/ak820ctl` | 0x800A | Firmware 1.10, Empfänger 0xFDFD mit 1.38 | `0xFF13`, Sessions `04 18 … 04 02 … 04 F0` | B | mittel |
| `hcristosm/ak820-linux` | 0x8009 | nicht genannt | `0xFF13` (Interface 3), `0xFF68` (Interface 2) | B | mittel |
| `fromagge/ak820-screen` | 0x800A | Firmware 1.16 | Config-Interface (`0xFF68`, `0xFF80`, `0xFF60` …), Bulk `0xFF67` | A | mittel |
| `jmanuelcorral/epoaura` | 0x800A, ISO-ES | Firmware 1.16 | TFT-Upload über „Screen" funktioniert, kabelgebundenes RGB wird nicht unterstützt | vermutlich A | niedrig |

Quellen: PR #12 `https://github.com/wsclx/ak820pro-modder/pull/12`; `https://github.com/allshouldbeready/ak820-pro-macos/blob/4b8b741/CHANGELOG.md` (Abschnitte 0.7.1-beta, „Hardware findings"); `https://github.com/DarkTiengo/ak820ctl/blob/2903211/README.md`; `https://github.com/hcristosm/ak820-linux/blob/5e910e0/README.md`; `https://github.com/fromagge/ak820-screen/blob/9030b81/PROTOCOL.md`; `https://github.com/jmanuelcorral/epoaura/blob/d3ea3f4/README.md`.

Aus der Tabelle folgt:

- Familie B tritt unter 0x8009 (allshouldbeready, hcristosm) und unter 0x800A auf, Familie A unter 0x8009 und unter 0x800A. Die PID taugt nicht als Schalter.
- Die Linux-Ports von gohv und TaxMachine sprechen Feature-Reports, also das Familie-B-Format. `docs/HANDOFF.md` §6.1 und `docs/PROTOCOL.md` nennen dieses Format „falsch", das gilt aber nur für das Mac-Board (Familie A). adamauthor (`docs/PROTOCOL.md`, Abschnitt „Firmware profiles") und allshouldbeready schreiben ausdrücklich, das Format sei für FF13-Boards gültig. Die offenen Issues zu 0x800A bei gohv (#5: nach Änderung der PID funktioniert das Licht; #6: defekte LCD-Bilder) und TaxMachine (#4) zeigen, dass diese Ports nur die PID 0x8009 kennen und beim Bild-Upload vermutlich eine andere Variante erwarten (mittel bis niedrig).
- Die Firmwarenummern sortieren sich nicht: 1.07 gehört zu A, 1.10 zu B, 1.16 wieder zu A. Ich vermute unterschiedliche Auslieferungsvarianten und keinen Versionssprung, kann das aber nicht belegen.
- Eine Tendenz „ANSI = B, ISO = A" passt zu den ANSI-Berichten von vespovios und allshouldbeready, aber kabelklaus beschreibt deutsche ISO-Geräte mit `0xFF13`. Als Regel taugt das nicht (niedrig).
- Unser Dokument sagt, `bcdDevice` folge der Firmware (`device.rs`, Feld `release_number`). adamauthor hält dagegen ausdrücklich fest, `0x0111` sei keine Firmware 1.11. Für das Mac-Board stimmt die Gleichung (0x0107 und 1.07), allgemein ist sie unbelegt. Der Kommentar von wsclx unter Issue #8 nennt „ANSI, Firmware 1.14"; jdlien schreibt, Firmware 1.14 ändere die PID auf 0x8099, und allshouldbeready hat ein 0x0114-Board mit PID 0x8009. Diese Angaben passen nicht zusammen (niedrig).

Die Auswahl nach HID-Collection (`0xFF13` vorhanden und `0xFF67` fehlt) ist die einzige Regel, die zu allen Datenpunkten passt. Das ist der Ansatz von `looks_like_feature_report_board` und `select_control` in `device.rs` sowie von #12.

Warnung aus `fromagge/ak820-screen`: Auf Firmware 1.16 (Familie A) löscht der Befehl `04 72` den gespeicherten Bild-Slot, nach dem Neustart zeigt das Gerät die Uhr. Die Legacy-Upload-Sequenz aus #12 darf deshalb nie an ein Board mit `0xFF67` gehen. #12 prüft den exakten Deskriptor, was dafür ausreicht (mittel).

#### 1.2 PID-Liste in der Konfiguration des offiziellen Web-Treibers

Aus `layout-classic-BIqq7haq.js` (SHA-256 `796f38ddf9032a2c2da1470f13457a9b5f6a4cae823eb62894911cf594ff856e`, abgerufen 24.10.2026 von `https://ajazz.driveall.cn/assets/layout-classic-BIqq7haq.js`). Die Einträge mit PID 0x8099, 0x8049 und 0x880D stehen auch in unserer Mai-Kopie `layout-default.js`, sind also nicht neu.

| PID | Name im Bundle | Bild / Sprache |
|---|---|---|
| 0x8009 | AK820 | AK820.png, deText |
| 0x800A | 820PRO und AK820 (zwei Einträge) | AK820.png (deText), AK820_es.png |
| 0x8049 | 820PRO | AK820.png, deText |
| 0x8099 | AK820 | AK820_US.png |
| 0x880D | AK820 | AK820_0d.png |
| 0x880E, 0x880F, 0x8810, 0x8811, 0x8812 | AK820 | en, de, fr, es, en (8812) |
| 0xFEFA, 0xFEFE | „2.4G Dongle" | AK820-Bilder |
| 0x8033, 0x8044, 0x8070, 0x809B, 0x80A0, 0x80B0, 0x80B1, 0x80B9 | AK820MAX (0x80B9: „Lightless") | andere Produktlinie |

Alle AK820-Einträge tragen `tftConfig` mit 128×128. Wo die Einträge Tastenlayouts mitliefern, gibt es nur Bilder für de, en, fr, es und US. Zu JIS oder ISO-UK steht dort nichts.

Unser `PRODUCT_IDS` in `crates/ak820-protocol/src/lib.rs` kennt 0x8009, 0x800A, 0xFEFE und 0x7140. Es fehlen 0x8049, 0x8099, 0x880D bis 0x8812, 0xFEFA und der von drei Projekten genannte 2.4-GHz-Empfänger 0xFDFD (gha-projects, DarkTiengo, allshouldbeready).

Verlässlichkeit: hoch dafür, dass die IDs im Bundle stehen. Niedrig dafür, welches Handelsprodukt zu welcher ID gehört. Das Bundle sagt nichts über Verkaufsnamen. Zudem gibt es ein „AK820" ohne Pro mit VID 0x320F und PID 0x505B (`dvino/ak820`, `HaritMT25/evision-rgb`), das mit unserem Filter auf VID 0x0C45 nicht kollidiert. Sonix-IDs werden außerdem anderweitig verwendet: `thibaultmiclo/aula-driver` nennt für die AULA EA75MAX die Kennung `0c45:800a`, und `lsusb` zeigt 0x800A als „Microdia Vivitar Vivicam3350B" (gohv-Issue #5). #12 prüft deshalb zusätzlich den Produktnamen.

#### 1.3 Platinen- und Panel-Revisionen

- Nach `fpb/ajazz-ak820-pro` Issue #4 steht auf manchen Platinen „V01", auf anderen „V02" aufgedruckt. Der Melder berichtet, das V02-Board habe mit QMK Bildfehler (weiß oder auf dem Kopf), und mit der Stock-Firmware sei ein hochgeladener Clip „verschoben" gewesen: unteres Bildstück oben, eine zusätzliche Spalte mit Müllwerten, um ein Pixel versetzt. fpb schreibt, dass derselbe Platinentyp mit unterschiedlichen LCD-Panels ausgeliefert wurde. Quelle: `https://github.com/fpb/ajazz-ak820-pro/issues/4` (mittel, Besitzerbericht und Antwort des Maintainers).
- `jdlien/ak820-pro` bestätigt zwei Panel-Varianten (eine davon umgekehrt oder farbinvertiert) für die eigene QMK-Firmware. Das betrifft die Stock-Firmware nicht direkt, zeigt aber, dass es Panel-Unterschiede gibt. Quelle: `https://github.com/jdlien/ak820-pro/blob/2e244df/README.md` (mittel).
- Der Web-Treiber überträgt Bilddaten nach einer Rotations-/Spiegelungsstufe (Funktion mit den Parametern `rotate` und `flip` im neuen Bundle) und liest `tftDirection` aus Byte 28 von `GET_DEVICE_INFO` (`0xFF` steht für „kein Bildschirm", laut `fromagge/ak820-screen` `PROTOCOL.md`). Ob die Stufe an `tftDirection` hängt, habe ich nicht geklärt (niedrig).

#### 1.4 Firmwarestände in freier Wildbahn

| Stand | Beleg |
|---|---|
| 1.07 | Mac-Board (`docs/HANDOFF.md`) |
| 1.10 | DarkTiengo (Gerät 0x800A); jdlien nennt 1.10 als ab Werk installierten Stand, der sich nicht mehr zurückspielen lässt |
| 1.13 | Stock-Image `AJAZZ_AK820PRO_PID_8009_V1.13_SN32F290.bin` im Repo `fpb/ajazz-ak820-pro` unter `StockFWBinaries/` |
| 1.14 | Image mit PID 0x8099; fpb und der Melder von fpb-Issue #4 berichten, die offizielle Software lehne ein damit geflashtes Board ab. Das Image stammt laut fpb aus einem Discord-Server |
| 1.16 | `fromagge/ak820-screen`, `jmanuelcorral/epoaura` |

Der Maintainer von fpb schreibt in Issue #4 (20.09.2026), Version 1.13 habe ihm der Hersteller geschickt, nachdem dieser auf der eigenen Website eine falsche Version bereitgestellt hatte. Sein eigenes Board habe dadurch die Firmware eines anderen Modells bekommen (falsche Tastenbelegung, verzerrtes LCD). Das ist eine einzelne Aussage eines Dritten (niedrig bis mittel), aber ein Grund, Firmware-Updates des Herstellers nicht in der App anzustoßen. Die vendoreigene OTA-Befehlsgruppe (siehe 2.3) sollte nicht implementiert werden.

#### 1.5 Layouts: JIS, ISO-UK und andere

- Ich habe keinen Beleg für ein JIS- oder ISO-UK-Modell gefunden, weder bei Händlern noch beim Hersteller. Die Produktseite von `ajazzstore.com` (selbst abgerufen) bietet ANSI, ISO-DE, ISO-FR und ISO-ES an. Der Web-Treiber hat Bilder für de, en, fr, es, US. Ein Rezensent testet ein ISO-ES-Gerät (`thephonograph.net`). Websuche, nur US-Index, daher ist „nichts gefunden" kein Gegenbeweis (niedrig).
- `docs/HANDOFF.md` §6.9b schreibt, das Gerät komme in „mindestens fünf Varianten, darunter ISO-UK und JIS" vor. Für die letzten beiden fehlt mir jeder Nachweis. Die Issues #5 (JIS) und die ISO-UK-Zeile in Issue #7 bleiben ohne Hardware nicht prüfbar.
- `kabelklaus/ajazz-ak820-pro` (README, „Known quirks") schreibt: Bei manchen deutschen ISO-Geräten ist die Taste unter „Entf" mit „Ende" bedruckt, sendet aber Home (`0x4A`). vespovios nennt dieselbe Taste im ANSI-Paint-Editor „Home", passend zur XML-Layoutdatei des Windows-Treibers. Unser `src/data/layouts/iso-de.json` führt dort Slot 107, Label „Ende", HID 77 (End), aus dem Export des Mac-Boards. Für Familie A ist das also bestätigt, für Familie-B-Boards möglicherweise falsch (mittel).
- ANSI-LED-Indizes der Familie B (aus der Windows-Treiber-XML, nach vespovios, bestätigt durch `DarkTiengo/ak820ctl` `src/proto.rs`): W = 39, A = 56, S = 57, D = 58, Enter = 85, Space = 94, Backspace = 103, Home = 117, PgDn = 121. Das ist ein anderer Indexraum als unsere Slots (in `iso-de.json`: W = 34, A = 49).

#### 1.6 Funk: 2.4-GHz-Empfänger und Bluetooth

- Der 2.4-GHz-Empfänger meldet sich in drei Projekten als `0C45:FDFD`. Er hat laut gha-projects keine Feature-Reports, sondern eine Schnittstelle `0xFF60` mit 32-Byte-Output-Reports. Format: `[Befehl][Länge][Chunk-Nr.][bis 28 Datenbytes][Summe]`, Prüfsumme in Byte 31 als Summe der Bytes 0–30 modulo 256. Über den Empfänger gehen Licht (Befehl `05`), Systemeinstellungen (`07`), Uhr (`0C`), Tastentabellen (`10`/`12`, 576 Byte in 21 Chunks), Makros (`09`), das Musikspektrum (`0B`, 21 Balkenhöhen) und die Akkuabfrage (`20`). Per-Key-Farben und Bilder gehen nicht, auch nicht im offiziellen Treiber. Quelle: `https://github.com/gha-projects/ak820-mac/blob/abd46c4/docs/PROTOCOL.md` und `docs/receptor-24g.md`; Bestätigung mit Hardware durch DarkTiengo (22.10.2026: Licht, Uhr, Info und Tastentausch F5 → Volume+ über den Empfänger). Verlässlichkeit mittel bis hoch.
- Die Akkuabfrage über den Empfänger liefert bei den getesteten Geräten dauerhaft 0, das gilt als „unbekannt" (gha-projects, allshouldbeready). Über Kabel gibt es bei Familie B keinen Akkuwert: adamauthors Board antwortet auf `20 01` mit `FF`.
- `0xFEFE` ist widersprüchlich beschrieben. Unser `docs/PROTOCOL.md`, gha-projects, allshouldbeready und adamauthor (`docs/WIRELESS.md`) nennen es Bluetooth ohne Konfigurationsschnittstelle. Der Web-Treiber führt 0xFEFE als „2.4G Dongle", ebenso `fromagge/ak820-screen` und `jmanuelcorral/epoaura` („ISO-ES receiver 0c45:fefe"). Vermutlich gibt es mehrere Empfänger-Revisionen. Ungeklärt (niedrig).
- Das TFT ist ausschließlich per Kabel erreichbar (fromagge, gha-projects, ErvinUrban). Der Vendor-Windows-Treiber kennt es ebenfalls nur per Kabel.
- Auf Linux hängen die Empfänger zeitweise (`goodguyartem/microdia-fix`, Watchdog in `DarkTiengo/ak820ctl`). Ob das unter macOS auch vorkommt, habe ich nicht gefunden (Lücke).

#### 1.7 Was Familie A und B nach den Quellen unterscheidet

| Funktion | Familie A (Web-Treiber, AA-Frames) | Familie B (Windows-Treiber, Feature-Reports) |
|---|---|---|
| Licht, 20 Modi | `SET_LED_EFFECT` (35), 16 B | `04 18`, `04 13`, 64 B Daten, `04 02`/`04 F0` |
| Per-Key (gespeichert) | `SET_CUSTOM_LED_DATA` (36), 512 B | Effekt `0x80`, dann `04 23`, 576-B-Bank in 9 Paketen |
| Per-Key (live) | Command 50 im Web-Treiber (`setMusicData`) | `04 20`, 8 Pakete, kein START, kein Speichern, rund 23 fps |
| Tastenbelegung | `SET_KEY` (34), 512 B | `04 11` / `04 27`, 576 B, Quelle gha-projects |
| Makros | `SET_MACRO` (37), Index plus Daten | `04 19` (löschen) und `04 15` |
| Systemeinstellungen | `SET_GAME_MODE` (33), 56 B | `04 17`, 64 B (kein Lesepfad) |
| Uhr | `SET_TEMPORARY_COMMAND_DATA` (52) | `04 28`, danach springt das Display zur Uhr |
| TFT | `0xAA 0x50`-Chunks mit 8-B-Header auf `0xFF67`, Ack `55 41` | `04 18`, `04 72 <Slot>`, rohe 4096-B-Blöcke auf `0xFF68`, Ack `01 5A 02`, `04 02` |
| Akku | `GET_DEVICE_INFO` Byte 17/18 | kabelgebunden nicht lesbar |

Quellen: `docs/PROTOCOL.md`; `https://github.com/gha-projects/ak820-mac/blob/abd46c4/docs/PROTOCOL.md`; `https://github.com/allshouldbeready/ak820-pro-macos/blob/4b8b741/docs/PROTOCOL.md`; `https://github.com/fromagge/ak820-screen/blob/9030b81/PROTOCOL.md`; Web-Treiber-Bundle (siehe 2.2).

---

### 2. Offizielle Software

#### 2.1 Windows-Treiber und Firmware

- Windows-Treiber: Es gibt weiter nur V1.0.0.5, und nur als ANSI-Variante. Die Epomaker-Datei heißt `AJAZZ_AK820Pro_Tripemode_0.85_inch_color_screen_RGB_V1.0.0.5_Keyboard_driver_20260107.rar.zip` (HTTP-Kopf: 28 342 096 Byte, `Last-Modified` 2. März 2026). Die Datei bei `ajazzstore.com` heißt `AJAZZ_AK820_Pro_Triple_mode_0.85inch_Screen_RGB_V1.0.0.5.zip` (28 335 390 Byte, `Last-Modified` 3. März 2026). Die Epomaker-Seite trägt das Datum 17.11.2025 („vV1.0 (ANSI Version Only)"). Quellen: `https://epomaker.com/blogs/software/ajazz-ak820-pro-driver`, `https://ajazzstore.com/blogs/software/ajazz-ak820-pro-driver`. Hoch für die Dateiangaben.
- Eine Websuche nennt auf der Seite `a-jazz.com` ebenfalls nur V1.0.0.5 für das Pro-Modell mit Bildschirm (und eine eigene V1.0.0.5 für MAX/MAX Plus). Die `a-jazz.com`-Seite lädt ihre Liste per JavaScript, ich konnte sie nicht selbst lesen. Mittel. gha-projects schreibt dazu, V1.0.0.5 sei die jüngste Version für das Bildschirm-Modell.
- `2.4G_upgrade.zip` (Epomaker-Seite vom 30.07.2024, 863 766 Byte) ist der Updater für den Empfänger. Eine neuere Fassung habe ich nicht gefunden.
- Changelogs oder Release Notes zu Treiber oder Firmware habe ich nirgends gefunden. Auch kein Firmware-Image auf Herstellerseiten. Lücke.
- Hashes: vespovios nennt für das Paket `98da8b9c801d413c1e84c4e08a2186922c1c297b9a173148123903b3867b8cf5` und für `DeviceDriver.exe` `14b6919b59b86f6bd1465e544032b41ca4f12ca35e36bd81d3ab33aca14a605a` (`docs/PER-KEY-RGB.md`, Commit `cc2a8df`). Unser Ledger (`docs/reverse-engineering/README.md`) führt für die V1.0.0.5-Exe `98439ddb0b38fc1b4ee63553618ce5f2df2614be36146a7f3d8735df63753b08`. Beide beanspruchen V1.0.0.5. Entweder sind es verschiedene Builds (ANSI-Epomaker gegenüber ANSI-Ajazz) oder unser Ledger ist falsch. Nicht aufgelöst.
- Was die statischen Analysen von vespovios, gha-projects und allshouldbeready zusammengenommen zum Windows-Treiber V1.0.0.5 ergeben: Er spricht Familie B. `config.xml` führt 0x800A wie 0x8009 (`gif_maxframes=140`, `gif_headlength=256`), `cmd_delaytime` beträgt 35 ms (Pause vor und nach jedem Befehl), die Tastentabelle hat 576 Byte, der Bild-Slot ist `GetCurSel() + 1`, und `rgb-keyboard.xml` deklariert `gif_count="1"`. Mittel bis hoch.

#### 2.2 Web-Treiber (ajazz.driveall.cn)

- Die Startseite `https://ajazz.driveall.cn/` hat laut HTTP-Kopf `Last-Modified: Sat, 24 Oct 2026 07:55:00 GMT`. Sie lädt jetzt `main-CbiUTl4Y.js`, `layout-classic-BIqq7haq.js` (3,3 MB), `vendor-lqQM5nv1.js` und `rolldown-runtime-Dd_uD5pT.js`. Die Dateien aus unserem Ledger (`index-CGDyjcPg.js`, `layout-default-DElMT--A.js`) liefern HTTP 404. Das Rezept im Ledger zum erneuten Abruf funktioniert nicht mehr. Hoch.
- Die Seiten für Französisch und Spanisch sind unverändert (Last-Modified 28.03.2026 und 02.02.2026). Das Französisch-Bundle `layout-default-kW0cplY3.js` ist byte-identisch zu unserer Kopie (SHA-256-Präfix `7717a19e…`). Hoch.
- Vergleich der Befehlstabelle (`HX`) gegen unsere Mai-Kopie `default-protocol.js`: 52 Einträge, davon keiner entfernt oder geändert. Neu sind 14: `GET_SIDE_LIGHT` (29), `GET_TRIGGER_KEY` (30), `SET_SIDE_LIGHT` (45), `SET_TRIGGER_KEY` (46), `GET_DOT_MATRIX_CONFIG` (61), `SET_TFT_MATERIAL` (77), `OTA_GET_DEVICE_SYSTEM_INFO` (128), `OTA_VERIFY_FIRMWARE_INFO` (129), `OTA_DEVICE_ENTER_BOOT` (130), `OTA_SEND_FIRMWARE_INFO` (131), `OTA_GET_DEVICE_SN` (132), `OTA_SWITCH_APP_PARTITION` (133), `OTA_SET_DEVICE_SN` (134) und `SET_PROFILE` (144). Hoch für die Differenz, mittel für ihre Bedeutung beim AK820 Pro (viele Befehle gehören vermutlich zu anderen Produktlinien wie den Magnetic-Axis-Tastaturen).
- Der TFT-Pfad ist unverändert: Header `AA 50 idx total 50 06`, also die Magic `0x0650`, die wir seit 0.8.x senden. Das bestätigt `docs/HANDOFF.md` §6.9h. Der Header baut sich weiter aus `min(delay * 5, 255)`; siehe 2.4.
- `wheelKeys` kommt in keinem der AK820-Einträge vor (geprüft pro Konfigurationsobjekt). Die Aussage in `docs/HANDOFF.md` §6.9a/§6.9i, der Drehknopf sei nicht umbelegbar, gilt weiter. Hoch.
- Das Bundle enthält wasm-bindgen-Glue mit den Bindungen `sendFeatureReport`/`receiveFeatureReport`. Der Glue steht auch in unserer Mai-Kopie von `layout-default.js`, ist also nicht neu. Als Filter für das Config-Interface nennt das Bundle die Usage Pages 65384, 65408, 65376, 65280, 65281, 65307 (`0xFF68`, `0xFF80`, `0xFF60`, `0xFF00`, `0xFF01`, `0xFF1B`) und standardmäßig 65383 (`0xFF67`) für Bulk. `0xFF13` (65299) kommt als Zahl nicht vor. Daraus folgt vermutlich, dass der Web-Treiber Familie-B-Boards nicht anspricht. Das ist eine Ableitung (niedrig).
- Für Pinning: SHA-256 der neuen Hauptdatei oben unter 1.2. Die Hashes von `main`, `vendor` und `rolldown-runtime` habe ich nicht ins Ledger geschrieben.

#### 2.3 Bisher nicht genutzte Befehle des Web-Treibers

| Befehl | Fundstelle im Bundle | Inhalt | Stand bei uns |
|---|---|---|---|
| 50 (`GET_LED_DATA`) | `setMusicData` | Wird als Schreibbefehl benutzt: Daten in Chunks von `reportCount − 8` mit Adresse pro Chunk (Datenlänge nicht ausgelesen, vermutlich 512 Byte als 128 × `[Index, R, G, B]` wie bei `04 20`), Timeout 50 ms (2000 ms bei `frameVersion == 1`), im Mai-Bundle schon enthalten | nicht implementiert |
| 53 (`SET_MUSIC_DATA`) | `setMusicDataV1` | Frame von `reportCount` Byte (Default 32): `AA 35 00 p1 p2 p3`, ab Byte 6 21 Balkenhöhen, Byte 31 = Summe der Bytes 0–30. Das Gerät zeichnet die Balken selbst. Entspricht dem Befehl `0B` des Empfängers bei gha-projects | nicht implementiert |
| 52 (`SET_TEMPORARY_COMMAND_DATA`) | `setTftDateTime`, `setTftScreenInfo` | Uhr (10 B) wie bei uns. Statistik (24 B): Byte 6 = `0x5A`, 12 CPU %, 13 CPU °C, 14 GPU %, 15 GPU °C, 16–18 Temperaturen, 19 Wettercode 0–23, 20 Luftfeuchte | Konstante da, Statistik nicht implementiert |
| 77 (`SET_TFT_MATERIAL`) | `setTftMaterial` | Neu. Header `AA 4D idx_lo idx_hi total_lo total_hi id_lo id_hi`, Material-ID 0–65535, 4096-B-Chunks. Zweck unklar | nein |
| 144 (`SET_PROFILE`) | `setProfile` | Neu. `AA 90 n`, Erfolg wenn `Antwort[2] == n` | nein; `docs/PROTOCOL.md` führt „Onboard profile switch" als nicht dekodiert |
| 128 (`OTA_GET_DEVICE_SYSTEM_INFO`) | `getDeviceSystemInfo` | Neu. 56 Byte: Bootloader- und App-Version (BCD wie bei `GET_DEVICE_INFO`), Hersteller, Produkt, laufender Bereich (`0x10` App, `0x30` Bootloader), `workMode` | nein |
| 250 (`GET_DEVICE_NOTIFY`) | `startDeviceStateListener` | Eingehende Reports `55 FA <Typ> …`; 2.4G-Schlaf `55 FC 06`, Aufwachen `A6 FF 01` | Konstante als Backlog notiert, kein Listener |
| `GET_DEVICE_INFO` Byte 20–21, 24–29 | `getDeviceInfo` | `axisInfo` (20–21), `gifMaxFrames` (24–25), `ledMaxFrames` (26–27), `tftDirection` (28), `rtPrecision` (29). Im Mai-Bundle schon enthalten | wir lesen 16, 17, 18, 19, 22–23, 30 (`system.rs`) |

Die Statistik-Seite zeigt der Web-Treiber nur bei Geräten mit `isShowTftInfo`. Bei den AK820-Einträgen ist das Flag nicht gesetzt, gesetzt ist es bei Einträgen mit dem Namen `AJAZZ_KEYBOARD_Y`. `fromagge/ak820-screen` weist ebenfalls darauf hin, dass die Firmware die Statistik eventuell ignoriert. Ob der AK820 Pro eine solche Seite zeichnen kann, ist offen.

Die OTA-Befehle (129–134) dienen dem Firmware-Update. Ich empfehle, sie nicht zu implementieren (siehe 1.4).

#### 2.4 TFT-Verzögerungseinheit: 2 ms, nicht 5 ms

Unser Code (`DELAY_UNIT_MS = 5` in `crates/ak820-protocol/src/commands/tft.rs`, Kommentar und `docs/PROTOCOL.md`) liest aus dem Web-Treiber `header[i + 1] = delay * 5` die Aussage „Einheit 5 ms". Laut `fromagge/ak820-screen` ist `delay` im Treiber aber die GIF-Frameverzögerung in Hundertstelsekunden. Eine Hundertstelsekunde (10 ms) ergibt dann Header-Wert 5, also eine Einheit von 2 ms. Den Typ von `delay` im Bundle habe ich nicht selbst nachverfolgt.

Belege für 2 ms:

- `fromagge/ak820-screen` `PROTOCOL.md`: „units of 2 ms (vendor: gif_centiseconds * 5)". Dieses Projekt hat denselben Web-Treiber ausgewertet und auf Firmware 1.16 (Familie A) gemessen, dass das Display nicht schneller als rund 60 ms pro Frame zeichnet (100 Frames mit 10 ms laufen in ca. 6 s, mit 100 ms in ca. 10 s).
- Familie B, vier Quellen: allshouldbeready (`PROTOCOL.md`: „2-ms units, clamped to 1..=255"), vespovios (`COMPATIBILITY.md`: „2 ms units"), `DarkTiengo/ak820ctl` (`screen_blob`: `ms / 2`), gha-projects (`docs/PROTOCOL.md`: „2 ms units (minimum 1)", der offizielle Treiber begrenze auf 30–500 ms).

Folgen bei 5 ms für uns: Ein GIF mit 100 ms pro Frame ergibt Header 20, also tatsächlich 40 ms. Die Firmware hebt es auf etwa 60 ms, das Bild läuft 1,7-mal zu schnell. Bei 200 ms ergibt sich Header 40, tatsächlich 80 ms, also 2,5-mal zu schnell. Der Höchstwert pro Frame läge bei 510 ms statt 1275 ms.

Verlässlichkeit: hoch für „2 ms bei den genannten Boards", mittel für das Mac-Board (Firmware 1.07), weil dort nicht gemessen wurde. Prüfung mit Stoppuhr siehe (c).

---

### 3. Community-Projekte und Forks

#### 3.1 Forks von `wsclx/ak820pro-modder`

| Fork | Befund |
|---|---|
| `adamauthor/ak820pro-modder` | Stand 24.10.2026, Version 0.9.8. Branch `fix/macos-legacy-hid` = PR #12 (4 Commits, Kopf `e217a42`). Weitere Branches: `feature/feature-tft-slot-rgb` (TFT-Slot 1 und Per-Key-RGB, Kopf `0c7f63a`), `feature/unified-macos`, `feature/gestures-profiles-music`, `feature/smart-lighting-anime-pomodoro`. Auf `main` stehen jüngste Änderungen: „Finish FF13 writes with SAVE, wait for busy keyboard (0.9.6)" (`a1f0933`), „Remove smart lighting, Pomodoro and the anime screen (0.9.7)" (`fb796a6`), „Write to FF13 keyboards only on Apply (0.9.8)" (`efb442f`). Kann: Familie-B-Lichtprotokoll mit START/SAVE/END, Bild-/GIF-Upload mit Ack, Uhr, Schlaf, hidutil-Remapping pro Gerät mit Baseline und Undo, Mac-Makrorekorder mit Accessibility-Prüfung, Drehknopf-Gesten über Mac-seitige Umleitung, App-Profile. Das meiste ist host-seitig. adamauthor schreibt am 24.10., er halte das Upstream-Repo für inaktiv und habe aufgehört, dorthin zu pushen. |
| `vespovios/ak820pro-modder` | Standardbranch `feature/ak820-800a-macos`, Kopf `cc2a8df` (23.10.2026). Familie-B-Unterstützung (ANSI), Slot-1-Korrektur, Per-Key-RGB mit Bank und Profilen, Menüleiste, Setup-Profile, Dokumente `COMPATIBILITY.md` und `PER-KEY-RGB.md`. Die Daten sind detailliert und teils per statischer Analyse des Windows-Treibers belegt (Adressen im Binary, Hashes). Hardwarebestätigung vom Autor für ANSI-Board. |
| `matsantiago/ak820pro-modder`, `0x5k/ak820pro-modder-upstream-archive` | Beide identisch mit `main` (0 voraus, 0 zurück). Nichts zu holen. |
| `0x5k/ak820pro-modder` (eigenes Repo, nicht der Fork) | Rust-CLI nur für TFT und Uhr, 0x8009, Mac. 0x5k ist die Person, die Issue #8 gemeldet hat. Das README berichtet: Dieses Board behalte „alte Pixel hinter kurzen Uploads". Um ein altes GIF, schwarze Frames oder eine „wandernde weiße Linie" zu vermeiden, füllt die CLI immer eine 132-Frame-Bank (1057 Blöcke zu 4096 Byte, rund 2,5 Minuten). Mittel (Einzelquelle, wahrscheinlich Familie B). |

Quellen: `https://github.com/adamauthor/ak820pro-modder`, `https://github.com/vespovios/ak820pro-modder`, `https://github.com/0x5k/ak820pro-modder`.

#### 3.2 Eigenständige Mac- und Plattformprojekte

| Projekt | Stand | Was es kann, das wir (noch) nicht können |
|---|---|---|
| `gha-projects/ak820-mac` (Python/Swift, MIT) | `abd46c4`, 17.10.2026 | Familie B plus 2.4-GHz-Empfänger 0xFDFD. Tastenbelegung und Makros im Gerät (`04 11`/`04 27`/`04 15`, hardwaregetestet auf 0x800A), Per-Key-Farben, Live-Per-Key (`04 20`), „Musikmodus" über einen Core-Audio-Tap (`musica/espectro.swift`, 21-Band-Spektrum, ab macOS 14.2), „Konfiguration erneut senden" aus einem lokalen Shadow-State, Werksreset mit Undo. Dokumentiert den Windows-Treiber ausführlich (`docs/PROTOCOL.md`). Mittel bis hoch. |
| `allshouldbeready/ak820-pro-macos` (baut auf dem Modder-Code auf, kein GitHub-Fork) | `4b8b741`, 13.09.2026 | Familie B (bcdDevice 0x0114): Licht, Uhr, Systemeinstellungen (`04 17`: Win-Taste, Alt+F4, Alt+Tab, Fn-Schalter, Schlaf, Tastenreaktion), TFT-Upload mit 256-Byte-Header (ein früherer 4096-Byte-Header erzeugte 15 weiße Zeilen), Bluetooth-Akku über macOS-Eigenschaft. Modell „zuletzt gelesen oder zuletzt gesendet" (`docs/CURRENT_STATE.md`). Mittel bis hoch. |
| `DarkTiengo/ak820ctl` (Rust, Windows/Linux) | `2903211`, 24.10.2026 | Kabel und Empfänger. Live-Per-Key mit ca. 15 Hz („não salva"), HyperHDR-Kopplung, Watchdog für hängende Empfänger, Videos auf dem TFT, Hooks. Mittel (Hardware 22.10. in Windows 11 getestet, Linux nicht). |
| `fromagge/ak820-screen` (Web, Python, MIT) | `9030b81`, 22.10.2026 | Familie A, Firmware 1.16: GIF/Video/Text auf dem TFT, `info`, Uhr, Verlaufsspeicher. Beschreibt, dass TFT-Uploads auf 1.16 nur im RAM liegen. Mittel. |
| `jmanuelcorral/epoaura` (Tauri/Svelte) | `d3ea3f4`, 15.09.2026 | ISO-ES-Empfänger mit Rücklesen der Beleuchtung. Mittel. |
| `hcristosm/ak820-linux`, `deddroid/ak820-rs`, `AaditPani-RVU/aklink`, `ErvinUrban-Bratislava/ak820pro-linux`, `imgunjan/ak820-linux-studio`, `PlexTDM/ak820` | Oktober 2026 | Linux und Windows, durchweg Familie B (deddroid: „Firmwares exposing 0xFF67 use a different protocol, not supported"; verweist auf unser Repo für diese Variante). `deddroid/ak820-rs` führt im README an, der Code sei mit Claude geschrieben worden und von deddroid getestet. |
| `KORTY-DEV/korty-keyboard-studio`, `RanilNassyrov/AK820-Studio` | Oktober 2026 | Windows. Für uns nur als Protokollnachweis nützlich. |
| `CaioSilvaDev/clak820` | 23.10.2026 | Zeigt Claude-Code-Verbrauch auf dem TFT; Familie B (`0xFF13`), Ack-Muster `01 5A 02`. |
| `kabelklaus/ajazz-ak820-pro` (WebHID, MIT), `Beattrey/ajazz-ak820-config` (WebHID, MIT, 19.05.2026), `gusleig/ajazz-ak820-pro-mac` (Python, MIT, 01.07.2026), `sellerbto/EPOMAKER-Ajazz-AK820-Pro`, `MemerGamer/AjazzAk820ProUnofficialWebdriver` | Mai bis August 2026 | Frühere Reverse-Engineering-Arbeiten, überwiegend Uhr, Licht und Bildupload der Familie B. |
| `gohv/EPOMAKER-Ajazz-AK820-Pro` (28 Sterne), `TaxMachine/ajazz-keyboard-software-linux` (50 Sterne) | letzter Push 14.04.2026 und 28.02.2025 | Beide veraltet. Offene Issues zu 0x800A (gohv #5, #6; TaxMachine #4) und Schlafmodus (gohv #4). |

Keines dieser Projekte kann die Familie-A-Funktionen (Onboard-Keymap im 512-B-Format, Makros mit Index-Layout, TFT über `0xAA 0x50`) besser als wir. Unsere Stärken gegenüber den Familie-B-Projekten bleiben Keymap/Makro-Editor, Audio-Reaktivität, Automations und iCloud-Sync.

#### 3.3 QMK, VIA und OpenRGB

- `fpb/ajazz-ak820-pro` (GPL-2.0, 95 Sterne, `161a36f`, 27.09.2026): QMK-Portierung für HFD80CP100 (SN32F299-Klon), mit LCD, Per-Key-RGB, Bluetooth/2.4G (CH582F-Treiber), Uhr (RTC D8563F), Flash-Speicher (PY25Q128HA). Stock-Firmware-Images 1.13 und 1.14 als Rückweg. Bootloader: Pins unter der Leertaste kurzschließen (VID/PID 0x0C45/0x7140). Verlässlichkeit hoch für die Beschreibung, nicht von uns getestet.
- `jdlien/ak820-pro` (`2e244df`, 23.10.2026): gehärtete Abspaltung („Jackrabbit"). Nennt 50–52 Stunden Akkulaufzeit mit voller Beleuchtung und mehrere Betriebsarten. Flashen löscht das emulierte EEPROM. Stock-Firmware lässt sich nicht auslesen. Warnung: v1.14 ändert die PID auf 0x8099.
- Mit QMK-Firmware funktioniert unsere App nicht, weil das Stock-Protokoll dann fehlt. Das gehört in README und `docs/INSTALL.md` als Hinweis (siehe (b) Nr. 13).
- `dvino/ak820` (QMK/Vial) betrifft ein anderes Gerät: AK820 ohne Pro, VS11K28A, VID/PID 0x320F:0x505B. `arafatamim/OpenRGBAK820Plugin` (`7fd74bc`, 04.10.2026) betrifft den AK820 Max Plus. Beides sind nicht unsere Boards.
- OpenRGB: Eine Suche ergab keinen Merge Request für den AK820 Pro. Die SignalRGB-Foren haben Threads, in denen der AK820 Pro nicht als Gerät erscheint (Lücke, weil die Suche nur den US-Index sieht).
- Reddit- oder Forenthreads zu Flash-Verschleiß oder Firmwareversionen habe ich nicht gefunden (Lücke).

---

### 4. Bekannte Probleme

#### 4.1 Flash-Verschleiß durch häufige Schreibzugriffe

Die Aussagen sind uneinheitlich und gelten für verschiedene Boards.

- adamauthor (PR #12, Kommentar vom 24.10., Familie B, bcdDevice 0x0111): Ohne SAVE (`04 02`) werden mehrere Lichtänderungen im Abstand von 300 ms still verworfen oder mit `FF` beantwortet, ein direkt folgender Bild-Upload verliert seinen ersten Block. Mit SAVE antworten alle Schreibvorgänge mit `01` und brauchen etwa 0,3 s. Außerdem: Er setzte „static green" mit SAVE, dann „static red" ohne SAVE (END antwortete `FF`) und schaltete die Tastatur vollständig aus. Sie kam rot zurück. Daraus schließt er, es gebe auf dieser Firmware keinen reinen RAM-Schreibpfad für Licht. Seine Schlussfolgerung für die Praxis: „a few writes per day, not per second". Seine alten Funktionen hätten täglich 100 bis 300 Lichtschreibvorgänge und bis zu rund 90 automatische Bild-Uploads ausgelöst. Er hat sie entfernt und die Automatik für Schieberegler auf FF13 abgeschaltet (Commits `a1f0933`, `efb442f` in seinem Fork). Er erwähnt selbst, dass er nicht ausschließen kann, dass die Firmware erst beim Abschalten schreibt. Hoch als Messung (Board 0x0111), mittel als Deutung.
- Einschränkung zu dieser Deutung: In seiner Versuchsanordnung wurde in beiden Fällen `04 F0` gesendet. gha-projects bezeichnet `04 F0` als „Save to flash" und `04 02` als Commit mit Prüfsumme. Ob ein Schreibvorgang ganz ohne `04 F0` im RAM bliebe, hat meines Wissens niemand getestet (niedrig, Hypothese).
- Live-Pfade ohne Speichern: Bei Familie B beschreiben zwei Quellen unabhängig voneinander `04 20` (Byte 8 = 8, acht Pakete, `04 02`) als Pfad „Per-key colors live (not saved; music mode)": gha-projects (Funktionen `0x4329e0` Musik und `0x427e10` Vorschau im Windows-Treiber, statisch) und `DarkTiengo/ak820ctl` (Kabel, ca. 15 Hz, Doku-Kommentar „não é salvo"; Hardware 22.10.2026). Nach gha-projects fällt die Tastatur zum gespeicherten Effekt zurück, sobald die Frames enden; bei etwa 23 Bildern pro Sekunde (5 ms um Befehle, 2 ms zwischen Paketen). vespovios nennt `00427E10` ebenfalls als „separate live-colour path". Verlässlichkeit mittel bis hoch für die Existenz des Pfads, niedrig für „schreibt nichts ins Flash" (niemand hat nach Ausschalten geprüft).
- Für Familie A hat der Web-Treiber die Pfade über Command 50 (`setMusicData`) und Command 53 (`setMusicDataV1`), beide schon in unserer Mai-Kopie enthalten (siehe 2.3). Dass sie nichts speichern, ist nicht belegt; die Namen („Music") und der Timeout von 50 ms sprechen dafür. Unser Audio-reaktives Licht nutzt dagegen `SET_CUSTOM_LED_DATA` (36) (`src-tauri/src/audio_reactive.rs`, Kommentar: „Connection::set_custom_led + apply_lighting(Custom)", 15 fps). Ob 36 auf dem Mac-Board persistiert, ist offen.
- Firmware 1.16 (Familie A): TFT-Uploads mit `0x50` liegen im RAM und gehen beim Abziehen des Kabels oder beim Moduswechsel verloren (`fromagge/ak820-screen` `PROTOCOL.md`, Abschnitt „Persistence"; dort durch Mitschnitt des Web-Treibers belegt). Bei Familie B trägt der Upload-Knopf des Windows-Treibers laut gha-projects (`docs/pantalla.md`) die Beschriftung „Save data and upload boot animation". Das legt nahe, dass das Bild dort als Bootanimation im Gerät liegt; das ist eine Ableitung, gemessen hat es niemand außer adamauthor (siehe oben). Mittel bis niedrig.
- Unser `src-tauri/src/tft_memory.rs` lädt nach Licht-, Keymap-, Makro-, Per-Key- und Systemschreibvorgängen den gemerkten Clip erneut hoch (Entprellung 750 ms). Das Modul führt selbst eine offene Frage zu seiner Notwendigkeit. Falls Uploads auf dem Mac-Board persistent sind, verstärkt das den Verschleiß bei jedem Licht-Apply um bis zu mehrere MB. Falls sie nur im RAM liegen, ist es harmlos für den Flash, aber der Clip überlebt keinen Neustart des Boards. adamauthor überspringt automatische GIF-Restores auf dem Familie-B-Pfad, weil das Display dort die Lichtänderung überlebt.
- Flash-Haltbarkeit: Ich habe keinen belegten Zahlenwert gefunden und nenne keinen (Lücke). Das Datenblatt des PY25Q128HA ist bei fpb verlinkt, ich habe es nicht ausgewertet. Unklar ist auch, wo die Beleuchtung gespeichert wird (MCU-Flash/emuliertes EEPROM, externer SPI-Flash). jdlien nennt für QMK einen Wear-Leveling-Mechanismus, der für die Stock-Firmware nichts beweist.

#### 4.2 TFT-Artefakte (#1, #2)

Keine externe Quelle erklärt „nur obere Hälfte" (#1) oder „Rauschlinien" (#2) auf Familie A. Verwandte Beobachtungen:

- allshouldbeready: Ein 4096-Byte-Header statt 256 Byte (Windows-Treiber: `gif_headlength="256"`) erzeugte 15 weiße Zeilen vor jedem Bild (Familie B).
- gha-projects (`docs/pantalla.md`, ältere Notiz vom 17.10., später gelöst): Ohne 35 ms Pause vor jedem Befehl ignorierte das Gerät das abschließende `04 02`, der Bildschirm blieb im „Loading"-Zustand. Zwischenzeitlich zeigten Tests verschobene oder gestreifte Bilder, wenn Daten eintrafen, während der vorherige Block noch gespeichert wurde. „Empfangen" bedeute nicht „gespeichert".
- 0x5k: Das Gerät behalte alte Pixel hinter kurzen Uploads; eine „wandernde weiße Linie" tauchte wieder auf.
- fpb-Issue #4: verschobenes Bild, eine Spalte Müll, Stock-Firmware, nach QMK-Experimenten (unklare Ursache).
- Fließkontrolle: PR #10 führt Ack-gesteuerte Uploads für das TFT ein; Hardware-Bestätigung auf dem Mac-Board für #1/#2 steht aus.
- Auffüllen der Chunks: `device.rs` (`upload_tft_animation`, `report[1..].fill(0)`) füllt mit Nullen. Der Web-Treiber tut dasselbe (`new Uint8Array(D).fill(0)`), der Windows-Treiber füllt mit `0xFF`. Zum Web-Treiber gibt es also keinen Unterschied.

Als Hypothese für #2 taugt die Beobachtung von 0x5k („Rest vom vorherigen Upload bleibt stehen"), als Hypothese für #1 keine der Quellen. Ich halte beides für niedrig verlässlich.

#### 4.3 Akkuanzeige

- Familie A: Bytes 17 (Ladestand) und 18 (Ladezustand) von `GET_DEVICE_INFO`. Das bleibt unsere Quelle.
- Familie B über Kabel: Es gibt keinen Lesepfad. Die Anfrage `20 01` antwortet auf adamauthors Board mit `FF`, auf allshouldbeready-Hardware gar nicht. Der Windows-Treiber fragt den Akku nur über den Empfänger ab (Modus 2), und die Antwort ist laut gha-projects und allshouldbeready oft 0 („keine Messung").
- Bluetooth: allshouldbeready liest den Ladestand nur dann, wenn macOS selbst ihn für das gekoppelte Gerät veröffentlicht.
- Die App sollte keine Null als 0 % anzeigen; #12 handhabt das so.

#### 4.4 Uhrseite

- Familie A: Uhr über Command 52 mit zehn Byte. Nach `fromagge/ak820-screen` gilt `5A 01 5A YY MM DD hh mm ss wd`, bei 24 Byte Padding in einigen Beschreibungen. Unser Code sendet zehn Byte (wie der Web-Treiber).
- Familie B: `04 28`, danach schaltet der Bildschirm nach Aussage von gha-projects zurück auf die Uhr.
- Seitenwechsel am Gerät: Fn + Delete (aus unserer `docs/PROTOCOL.md`; 0x5k bestätigt das für 0x8009). Auch die Uhrseite verlangt laut 0x5k nach `sync-time` ein Umschalten per Fn + Delete.
- `SET_TFT_BUILT_IN_INDEX` (81) läuft bei 0x5k auf 0x8009 in ein Timeout (erwartet, schreibt das README).

---

### 5. Plattform

#### 5.1 Tauri und Abhängigkeiten

| Paket | Lockfile | aktuell (crates.io, 25.10.2026) |
|---|---|---|
| tauri | 2.12.3 | 2.12.5 (21.10.) |
| tauri-build | 2.7.2 | 2.7.4 |
| wry | 0.57.0 | 0.57.1 (23.10.) |
| tao | 0.37.1 | 0.37.2 (23.10.) |
| tauri-plugin-dialog | 2.8.2 | 2.8.3 |
| tauri-plugin-fs | 2.7.0 | 2.7.1 |
| tauri-plugin-global-shortcut | 2.4.1 | 2.4.1 |
| tauri-plugin-shell | 2.5.0 | 2.5.0 |
| hidapi (Rust) | 2.6.5 | 2.6.7 (27.08.) |
| screencapturekit | 2.1.0 | 11.1.0 (24.10.) |
| realfft | 3.5.0 | 3.5.0 |

- Änderungen laut `crates/tauri/CHANGELOG.md`: 2.12.4 behebt eine Panik in den IPC- und Asset-Handlern bei leerem URI-Pfad (z. B. `ipc:foo`) und verbessert eine iOS-Fehlermeldung. 2.12.5 betrifft NSIS-Installer unter Windows. Beide Versionen sind für macOS klein. Im Cargo.toml stehen Caret-Bereiche (`tauri = "2.12"`), `cargo update` hebt also nur die Lockdatei. Hoch.
- Ein `tauri-v3.0.0-alpha.5` erschien am 14.10.2026. Für uns zu früh.
- Für hidapi-rs 2.6.4 bis 2.6.7 habe ich keine Release Notes gefunden (die Release-Seite endet bei 2.6.3). Lücke. Das C-Projekt `libusb/hidapi` hat in 0.15.0 (18.05.2025) eine macOS-Option für die Kompatibilitätsversion hinzugefügt, nichts zu HID-Zugriffsrechten.
- Die Crate `hidapi` bietet einen Feature-Schalter `macos-shared-device` (geprüft in den Metadaten von 2.6.7). Unser `probe_interfaces` öffnet standardmäßig exklusiv (Kommentar in `device.rs` Zeile ~177). adamauthor öffnet die Vendor-Handles nicht-exklusiv und hat damit Schreibzugriffe getestet (`docs/MACOS-800A.md`). Mittel.

#### 5.2 ScreenCaptureKit und Audio-Capture

- `screencapturekit` (doom-fish) hat seit unserem Stand 2.1.0 (11.05.2026) neun Hauptversionen veröffentlicht, alle mit Brüchen. Das `CHANGELOG.md` verzeichnet für den Audio-Pfad: 9.0.0 macht `AudioBuffer`-Felder privat, weil die öffentlichen Felder „Slices über das Ende des Audioblocks hinaus" erlaubten. 11.0.0 entfernt die Swift-Audiobrücke, die jeden Buffer auf die gesamte Block-Buffer-Länge begrenzte, und prüft Zeiger und Länge pro Buffer. 11.0.1 (23.10.2026) unterstützt macOS 12.3 und meldet Audio-Capture vor macOS 13.0 als nicht verfügbar. 11.0.0 ergänzt die `SCStreamErrorCode`-Werte -3822 (`InsufficientStorage`) und -3823 (`NotSupported`) aus dem macOS-27-SDK. `from_preset` liefert jetzt ein `Result`. Welche dieser Brüche `crates/ak820-audio-reactive/src/capture.rs` treffen, habe ich nicht geprüft. Hoch für den Inhalt des Changelogs.
- `capture.rs` dokumentiert, dass SCStream auch für reines Audio das Screen-Recording-Recht braucht.
- Core-Audio-Process-Taps (macOS 14.2+) brauchen stattdessen die Berechtigung „System Audio Recording". Ein Tap wird auch ohne die Berechtigung erzeugt, liefert dann aber Stille; es gibt keine öffentliche API, um sie abzufragen oder anzufordern. Der Schlüssel `NSAudioCaptureUsageDescription` gehört direkt in die Info.plist, die Xcode-Build-Einstellung greift nicht. Quelle: Zusammenfassungen von Suchtreffern, die ich nicht selbst geöffnet habe (`releases.electronjs.org/pr/53646`, `docs.rs/crate/coreaudio/0.3.3`, `github.com/insidegui/AudioCap`, `github.com/makeusabrew/audiotee`). gha-projects nutzt dafür ein Swift-Hilfsprogramm (`musica/espectro.swift`) und schreibt im README, macOS frage nach der Berechtigung für Systemaudio, „nicht den Bildschirm". Niedrig bis mittel; vor der Umsetzung selbst prüfen.
- Unsere `src-tauri/entitlements.plist` enthält nur `device.usb` und zwei Hardened-Runtime-Ausnahmen, kein Audio-Entitlement, und im Repo gibt es keine eigene `Info.plist` mit Nutzungstexten.

#### 5.3 macOS 26/27 und HID

- Zu macOS 27 habe ich keine belegte Änderung für IOHIDManager, hidapi, Input Monitoring oder Core-Audio-Taps gefunden. Die Suche lieferte nur ältere Beobachtungen: Das Auflisten von Tastaturen per `IOHIDManagerCopyDevices` kann die Eingabeüberwachung auslösen, ein verweigertes Recht zeigt sich bei hidapi als `kIOReturnNotPermitted` beim Öffnen, und vendor-spezifische Geräte verhalten sich unter CoreHID teils anders als unter IOKit (Apple Developer Forums 804793, 735999, 775577; nur die Suchzusammenfassung gelesen). Lücke für macOS 27.
- adamauthor (`docs/MACOS-800A.md`): Das Öffnen der Vendor-Collections aus einer Prozess-Sandbox seiner Entwicklungsumgebung schlug mit „not permitted" fehl, außerhalb klappte es. Unsere `src-tauri/entitlements.plist` enthält keinen Eintrag `com.apple.security.app-sandbox`, die App läuft also ohne App Sandbox.
- ScreenCaptureKit steht laut Apple-Dokumentation nun auch für iOS, iPadOS, tvOS und visionOS 27.0 bereit (`developer.apple.com/tutorials/data/documentation/screencapturekit.md`), und es wird `SCContentSharingPicker` empfohlen. Für unseren Fall (Systemton) ergibt das keine Änderung. Eine Änderung für reines Audio unter macOS 27 habe ich nicht gefunden (Lücke).
- Unser Mindestsystem steht auf 11.0 (`tauri.conf.json`), das Feature `macos_13_0` von `screencapturekit` und die Audio-Capture-Funktionen verlangen mindestens 13.0, ein Core-Audio-Tap 14.2.

---

## (b) Priorisierte App-Verbesserungen

Die Priorität richtet sich nach Wirkung und nach Kosten der Prüfung. „Test" verweist auf Abschnitt (c).

### Priorität 1

#### 1. TFT-Verzögerungseinheit auf 2 ms prüfen und korrigieren

Dateien: `crates/ak820-protocol/src/commands/tft.rs` (`DELAY_UNIT_MS`, `MAX_FRAMES`-Nachbarschaft, Unit-Tests `encode_three_frame_delays`), `docs/PROTOCOL.md` (Abschnitt „Per-frame delay encoding"), `docs/HANDOFF.md`, GIF-Pfad in `tft_image`, Anzeige in `src/views/Tft.tsx`.
Issue: keines direkt; berührt #1 und #2 nur über die Abspielgeschwindigkeit.
Test: (c) Nr. 1. Wenn bestätigt: Einheit auf 2, Maximum pro Frame 510 ms, Minimum 1 Einheit, UI-Hinweis, dass das Display unter ca. 60 ms pro Frame ohnehin nicht schneller zeichnet (fromagge, DarkTiengo: ca. 16 Bilder pro Sekunde).

#### 2. Live-Pfad für Audio-reaktives Licht

Ziel: Das Audio-reaktive Licht soll nicht über den gespeicherten Pfad (`SET_CUSTOM_LED_DATA` 36) laufen, sondern über Command 50 (Familie A) und, falls #12 integriert wird, über `04 20` (Familie B). Ein Frame hat dann bei Familie A einen Weg ohne `apply_lighting(Custom)` und bei Familie B ist Audio-Reaktivität überhaupt erst möglich (bei #12 steht „unsupported on FF13").
Dateien: `crates/ak820-protocol/src/protocol.rs` (Konstante `LIVE_LED_DATA = 50`, `SET_MUSIC_DATA = 53`), `crates/ak820-protocol/src/device.rs` (neue Methode neben `set_custom_led`), `crates/ak820-protocol/src/commands/per_key_rgb.rs`, `src-tauri/src/audio_reactive.rs` (`TARGET_FPS`, Zeile ~54), Doku in `docs/PROTOCOL.md`.
Issue/PR: #3 (Flackern), #11 (ACK-Skip). #11 bleibt für die weiteren Schreibpfade sinnvoll, ersetzt aber den Live-Pfad nicht.
Offen: Die Struktur von Command 50 ist nur aus dem Bundle abgelesen (Chunk-Größe `reportCount − 8`, Adresse pro Chunk, dritter Parameter und letzte Paketkennung nicht eindeutig). Test: (c) Nr. 4 und 5. Für Command 53: 21 Balkenhöhen reichen für ein Spektrum bei einem einzigen Report pro Frame, die Firmware zeichnet selbst. Das ändert das Design (Gerät statt Host rendert), wäre aber die sparsamste Variante.

#### 3. Schreibbudget: Persistenz messen, Wiederhol-Uploads und Auto-Apply anpassen

Dateien: `src-tauri/src/tft_memory.rs` (Restore nach Licht, Keymap, Makro, Game-Mode, Preset; Entprellung 750 ms), `src-tauri/src/lib.rs` (`memory.schedule_restore()` in `apply_lighting` u. a., Zeile ~845), `src/views/Lighting.tsx` (`autoApply`, `scheduleApply`), `src/views/Presets.tsx`.
Vorgehen: Erst messen (c) Nr. 3/4/6, dann entscheiden. Falls Uploads und Lichtschreibvorgänge auf dem Mac-Board persistent sind: Restore nur noch auf Wunsch, Auto-Apply im Lichtdialog standardmäßig aus oder auf „Anwenden"-Knopf mit Zähler, Schreibzugriffe pro Stunde begrenzen. Falls sie nicht persistent sind: Restore bleibt, ein Hinweis erklärt, dass der Clip einen Neustart nicht übersteht.
Familie B: dort ist Auto-Apply nach den Daten von adamauthor von vornherein abzuschalten (er setzt in seinem Fork „Write on Apply only").

#### 4. `GET_DEVICE_INFO` vollständig auswerten, Frame-Limit aus dem Gerät

Dateien: `crates/ak820-protocol/src/commands/system.rs` (`DeviceInfoReport`: neu `axis_info` 20–21, `gif_max_frames` 24–25, `led_max_frames` 26–27, `tft_direction` 28, `rt_precision` 29), `crates/ak820-protocol/src/commands/tft.rs` (`MAX_FRAMES = 255` ersetzen durch `min(gif_max_frames, 140)`), Anzeige in `src/views/System.tsx`, `src/types.ts`.
Begründung: Der Web-Treiber warnt anhand von `tftMaxFrames` vor zu vielen Frames. Der Windows-Treiber begrenzt auf `gif_maxframes=140`. Wir lassen bis 255 zu. Die Magic `0x0650 × 4096 = 6 619 136` Byte im Chunk-Header gibt rechnerisch Platz für etwa 201 Frames (`(6 619 136 − 256) / 32 768`), das ist meine Ableitung und nicht belegt (niedrig).
Test: (c) Nr. 2.

#### 5. #12 (Familie B) ergänzen, bevor es integriert wird

#12 ist mit 28 geänderten Dateien umfangreich; ich habe den Code nicht im Einzelnen geprüft, nur die Beschreibung und die Kommentare gelesen. Aus den anderen Quellen ergeben sich folgende Punkte, die in #12 oder im Folgeschritt gehören:

- TFT-Ziel-Slot als Parameter, Standard 1. Drei Werte kommen vor: 3 (#12 bei `e217a42`), 1 (vespovios, gha-projects, `DarkTiengo/ak820ctl`), 2 (allshouldbeready). Der Windows-Treiber sendet `GetCurSel() + 1`, und `rgb-keyboard.xml` deklariert `gif_count="1"`. Dateien in #12: `crates/ak820-protocol/src/legacy_tft.rs`.
- Pause von 35 ms vor und nach jedem Befehl (Vendor-`cmd_delaytime`). vespovios hat denselben Effekt gemessen (SAVE wurde ohne Pause abgelehnt), gha-projects ebenfalls („ohne die Pause ignoriert das Gerät das letzte `04 02`").
- Semantik von `04 02` und `04 F0` klären (Prüfsumme/Commit gegenüber „Save to flash") und die Namen in #12 anpassen, bevor Benutzer auf Basis von „SAVE" Schlüsse zum Flash ziehen. Test: (c) Nr. 10.
- Live-Per-Key `04 20` für Audio-Reaktivität (siehe Nr. 2).
- Systemeinstellungen `04 17` (Win-Taste, Alt+F4, Alt+Tab sperren, Fn-Schalter, Schlafzeit, Tastenreaktion), schreibend und ohne Lesepfad. Der Zustand muss lokal gemerkt werden (so macht es allshouldbeready).
- #12 beschreibt Onboard-Tastenbelegung und Onboard-Makros für FF13 als nicht unterstützt. gha-projects stellt das infrage: Dort laufen Onboard-Tastentabellen (576 Byte, `04 11`/`04 27`) und Makros laut README hardwaregetestet auf einem 0x800A (`docs/teclas.md`). Das wäre ein eigener Schritt, der Zugang zu einem Familie-B-Board voraussetzt. Die Host-Fallbacks aus #12 (hidutil) blieben dann als Zusatz. Mittel.
- Produktname prüfen (Sonix-ID-Wiederverwendung, 1.2). Steht in #12.

Dateien im Repo: `crates/ak820-protocol/src/device.rs`, nach der Integration `legacy.rs`, `legacy_tft.rs`, `capabilities.rs`, `src-tauri/src/lib.rs`, `src/capabilities.tsx`, `src/views/Lighting.tsx`, `src/views/Tft.tsx`.
Issue/PR: #8 (Melder 0x5k, Familie B), #10, #12.

### Priorität 2

#### 6. PID-Tabelle und Erkennung erweitern

Dateien: `crates/ak820-protocol/src/lib.rs` (`PRODUCT_IDS`), `crates/ak820-protocol/src/device.rs` (Filter in `enumerate`), `src/views/Connect.tsx`, `docs/PROTOCOL.md` (Abschnitt „Known PIDs").
Inhalt: 0x8049, 0x8099, 0x880D bis 0x8812, 0xFEFA sowie 0xFDFD (Empfänger) aufnehmen, aber nur als „erkannt, Protokoll gemäß HID-Collections" anzeigen und nie allein anhand der PID Befehle senden. Namensprüfung („AK820") bei den nicht eindeutigen IDs. Ergänzung in `docs/PROTOCOL.md`: Tabelle der Familien aus 1.1, Hinweis, dass `bcdDevice` ≠ Firmware.
Issue: #8.

#### 7. hidapi: nicht-exklusives Öffnen und Update 2.6.7

Dateien: `Cargo.toml` (Workspace: `hidapi = { version = "2.6", features = ["macos-shared-device"] }`), `crates/ak820-protocol/src/device.rs` (Kommentar zu `probe_interfaces`).
Nutzen: Die App blockiert dann den Zugriff anderer Werkzeuge nicht (z. B. gleichzeitige CLI). adamauthors Tests sind ein Hinweis, dass Schreiben im geteilten Modus klappt. Test: (c) Nr. 12.

#### 8. Tauri und Plugins auf die aktuellen Patchstände ziehen

Dateien: `Cargo.lock` (`cargo update -p tauri -p tauri-build -p wry -p tao -p tauri-plugin-dialog -p tauri-plugin-fs`). Das ist unabhängig von einem Funktionsgewinn; enthalten ist die Korrektur einer IPC-Panik bei leerem Pfad. Danach CI-Lauf (`.github/workflows/ci.yml`) abwarten. Verwandt mit PR #10 („CI/Tauri security update").

#### 9. Audio-Capture: Berechtigung und Bibliothek

Option A: `screencapturekit` auf 11.x heben (Brüche, Sicherheitskorrekturen im Audio-Pfad), `crates/ak820-audio-reactive/src/capture.rs` und `Cargo.toml` anpassen, Mindestsystem 13 beachten.
Option B: Core-Audio-Process-Tap (ab macOS 14.2) mit Berechtigung „System Audio Recording" statt „Screen Recording", `NSAudioCaptureUsageDescription` in die Info.plist (`src-tauri/tauri.conf.json` bzw. eine ergänzte `Info.plist`), gegebenenfalls Eintrag in `src-tauri/entitlements.plist`. Vorbild: `musica/espectro.swift` in `gha-projects/ak820-mac` (MIT, Lizenz vor Übernahme von Code prüfen; wir übernehmen höchstens die Technik).
Empfehlung: erst Nr. 2 klären, danach B planen. Option B senkt die Hemmschwelle für Nutzer (Bildschirmaufnahme-Dialog fällt weg). Issue: #3 nur mittelbar.

#### 10. Nur lesende Zusatzbefehle: Bootloader-/App-Version und Profil

- `OTA_GET_DEVICE_SYSTEM_INFO` (128), nur lesend, für die Systemansicht (App-Version, Bootloader-Version, laufender Bereich, `workMode`). Keine anderen OTA-Befehle.
- `SET_PROFILE` (144): `AA 90 n`, Bestätigung `Antwort[2] == n`. Nur mit Hardwaretest (c) Nr. 8, bei Erfolg Profilumschalter in `src/views/System.tsx`, Eintrag im Backlog „Onboard profile switch" in `docs/PROTOCOL.md` schließen.
Dateien: `crates/ak820-protocol/src/protocol.rs`, `device.rs`, `commands/system.rs`, `src-tauri/src/lib.rs`, `src/views/System.tsx`.

#### 11. TFT-Artefakte (#1, #2): gezielte Hypothesen testen

Mit der Ack-Steuerung aus #10 erneut auf dem Mac-Board prüfen (c) Nr. 7. Falls #2 bleibt: Bank vollständig überschreiben (0x5k-Methode) testweise, nur zur Diagnose. Falls #1 bleibt: Anzahl der Chunks gegen `ceil(Größe / 4096)` und `totalChunks` im Header prüfen, `tft_presets`-Quadrants-Frame byteweise kontrollieren. Dateien: `crates/ak820-protocol/src/commands/tft.rs`, `device.rs` (`upload_tft_animation`), `commands/tft_presets.rs`. Issues #1, #2.

#### 12. Reverse-Engineering-Ledger und Dokumentation auf den Stand bringen

Dateien: `docs/reverse-engineering/README.md` (Ledger: neue Bundle-Namen und SHA-256 der Dateien vom 24.10.2026, Hinweis auf 404 der alten, Hash-Abweichung der Windows-Exe klären), `docs/PROTOCOL.md` (Verzögerungseinheit, Familienmatrix, PID-Liste, neue Befehle 50/53/77/128/144, Hinweis „`04 72` löscht den Slot auf Firmware 1.16"), `docs/HANDOFF.md` §6.1 (die Linux-Ports sind für Familie-B-Boards korrekt; die Aussage „wrong on macOS" gilt nur für Familie A), §6.9b (JIS und ISO-UK als „keine Hinweise auf reale Geräte").

### Priorität 3

#### 13. Hinweis auf QMK-Firmware und Firmware-Updates

README und `docs/INSTALL.md`: Mit QMK-Portierungen (`fpb`, `jdlien`) bleibt das Stock-Protokoll aus; die App erkennt sie nicht. Offizielle Firmware-Updates nicht über die App anbieten (siehe 1.4).

#### 14. Layoutdaten

`src/data/layouts/iso-de.json`: Slot 107 („Ende", HID 77) ist für das Mac-Board bestätigt, für Familie-B-Boards eventuell Home (kabelklaus, vespovios). Familie-B-Layouts gehören nach der architektonischen Regel in `docs/HANDOFF.md` §6.9b in eigene Layoutdateien mit eigenen Slot-/LED-Indizes (1.5). Issues #5 und #7.

#### 15. Statistik-Seite des Displays testen

`SET_TEMPORARY_COMMAND_DATA` (52) mit dem 24-Byte-Statistik-Payload an das Mac-Board senden und beobachten, ob das Gerät eine Seite mit CPU/GPU/Wetter zeigt. Falls ja, könnte #6 (Live-Presets) ohne Bild-Uploads und ohne Verschleiß auskommen. Ein CLI-Unterbefehl `tft stats` in `crates/ak820-cli/src/main.rs` genügt für den Test. Issue #6.

#### 16. Geräte-Benachrichtigungen und Empfänger

- `GET_DEVICE_NOTIFY` (250) als Eingangslistener (Schlaf, Aufwachen, Zustand), ersetzt Polling in `src/App.tsx` und den Backlog-Eintrag „Real LED indicators". Inhalte der Typen sind unbekannt, dafür ist ein Mitschnitt nötig.
- Empfänger-Betrieb (0xFDFD/FF60, 32-Byte-Reports mit Prüfsumme) für Licht und Uhr. Nur sinnvoll, wenn jemand eine Familie-B-Hardware mit Empfänger zur Verfügung hat.

---

## (c) Offene Fragen, die nur ein Hardware-Test klärt

Die Tests 1 bis 8 und 12 laufen auf dem Mac-Board (ISO-DE, 0x8009, Firmware 1.07, Familie A). Nr. 9 bis 11 brauchen ein Familie-B-Board.

1. Verzögerungseinheit: GIF mit 50 Frames zu 200 ms hochladen, Schleifendauer mit Stoppuhr messen. Erwartung bei 2 ms: 10 s. Bei 5 ms und Firmware-Minimum würde die Schleife deutlich kürzer laufen (ca. 4 s). Danach 500 ms pro Frame gegenprobieren. Betrifft Nr. 1 in (b).
2. `GET_DEVICE_INFO` roh ausgeben (Bytes 16 bis 30; dem CLI-Befehl `ak820 info` fehlt dafür eine Rohausgabe, die man kurz ergänzen müsste): Was stehen für `gifMaxFrames` (24–25), `ledMaxFrames` (26–27), `tftDirection` (28), `rtPrecision` (29) und `axisInfo` (20–21) auf dem Board? Passt `tftMaxFrames` = 255 aus `docs/HANDOFF.md` zu `gifMaxFrames`? Betrifft Nr. 4.
3. Persistenz des TFT-Uploads: Clip hochladen, Kabel abziehen, wieder anstecken, vollständig aus- und einschalten. Bleibt der Clip? (Firmware 1.16 behält ihn laut fromagge nicht.) Betrifft Nr. 3 und `tft_memory.rs`.
4. Persistenz von `SET_CUSTOM_LED_DATA` (36): Per-Key-Frame schreiben, Strom trennen (Kabel ab, Schalter auf Aus), prüfen, ob der Frame zurückkommt. Danach nach einer Minute Audio-Streaming wiederholen. Betrifft Nr. 2 und Nr. 3.
5. Command 50 und 53 auf Firmware 1.07: Antwortet das Board? Zeigt es die Frames an, wie schnell (Frames pro Sekunde), kehrt es nach Abbruch zum gespeicherten Effekt zurück, überlebt ein Frame den Stromverlust? Ein erster Versuch mit einem 512-Byte-Frame in Chunks zu 56 Byte reicht; dafür braucht das CLI einen kleinen Testbefehl (es hat bisher kein `raw`). Betrifft Nr. 2.
6. Löscht ein `SET_LED_EFFECT` den TFT-Inhalt immer noch (`tft_memory.rs`, Abschnitt „Open question")? Mit und ohne die Ack-Steuerung aus #10. Wenn nein, kann das Modul entfallen.
7. TFT-Artefakte #1/#2 nach der Ack-Steuerung: Verschwindet „nur obere Hälfte" mit dem Diagnose-Quadrants-Preset? Treten die Rauschlinien nach einem kürzeren Clip auf, der einen längeren ersetzt (0x5k-Beobachtung)?
8. `SET_PROFILE` (144) mit Wert 0 und 1 senden: Kommt eine Antwort mit gleichem Wert? Ändert sich `currentProfile` (Byte 19)? Gibt es überhaupt mehrere Profile am AK820 Pro?
9. Familie B: Bleibt eine Lichtänderung ohne `04 F0` im RAM (nur `04 18`, `04 13`, Daten, `04 02`)? Test: ändern, Strom trennen, schauen, ob der alte Wert zurückkehrt. Wahrscheinlich auf adamauthors oder vespovios' Board.
10. Familie B: Welcher TFT-Slot (1, 2 oder 3) wird beim Start gezeigt, und wie verhalten sich Uploads in andere Slots? Drei Projekte nennen drei Werte.
11. Familie B: Bringt `04 20` das Per-Key-Licht ohne Speichern (Aus-/Einschalten nach Streaming), und wie hoch ist die erreichbare Rate über macOS-hidapi?
12. Nicht-exklusives Öffnen (`macos-shared-device`) auf macOS 27: Funktionieren Schreiben und Lesen unverändert, und stört die gleichzeitige Nutzung des Web-Treibers im Browser nicht?
13. Zuordnung von `bcdDevice` zur Firmware (`GET_DEVICE_INFO` Byte 8–9) auf mehreren Boards. Wir haben nur den einen Datenpunkt 0x0107 = 1.07. Hilfreich wäre die Ausgabe von `ak820 list` (mit `release=`-Spalte aus #10) von der Gegenseite von Issue #8 (0x5k).
14. Statistik-Seite (Abschnitt 2.3 und (b) Nr. 15): Zeigt der AK820 Pro etwas, wenn Command 52 mit dem 24-Byte-Statistik-Payload kommt?
15. Empfänger an Familie A: Beantwortet der Empfänger `AA 10` auf `0xFF60` mit 32-Byte-Reports? Unser `CONTROL_USAGE_PAGES` enthält `0xFF60` bereits, ob die Frame-Länge 64 Byte dort reicht, ist ungeprüft.
16. Taste unter „Entf" auf dem Mac-Board: Sendet sie End (HID 77) wie in `iso-de.json`, oder Home? Ein Tastenmonitor genügt.

## Lücken

- Keine Changelogs zu Windows-Treiber, Web-Treiber oder Firmware gefunden. Keine offizielle Quelle für Firmware-Images.
- Kein Nachweis für JIS- oder ISO-UK-Modelle.
- Kein Zahlenwert zur Flash-Haltbarkeit, und nicht geklärt, wo welche Einstellung liegt.
- Keine Reddit- oder Forenthreads gefunden; die Suche sieht nur den US-Index.
- Keine Release Notes für hidapi-rs 2.6.4 bis 2.6.7.
- Keine belegte Änderung bei HID, Input Monitoring, ScreenCaptureKit-Audio oder Core-Audio-Taps unter macOS 26/27.
- Nicht geprüft: Ob die Community-Projekte in 3.2 die behaupteten Hardwaretests tatsächlich durchgeführt haben. Nicht gelesen: Quellcode dieser Projekte über die genannten Dateien hinaus.
- Der Web-Treiber enthält einen WebAssembly-Anteil, den ich nicht entpackt habe.
