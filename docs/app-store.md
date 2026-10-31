# App Store (iOS + watchOS)

Texte für App Store Connect (App-ID 6828874328). Sprachen: Englisch (Primär), Deutsch.

## Allgemein

| Feld | Wert |
|---|---|
| Kategorie | Health & Fitness (Sekundär: Utilities) |
| Preis | kostenlos, keine In-App-Käufe |
| Altersfreigabe | 4+ (alle Fragen „Nein“) |
| Datenschutz-URL | https://uwu.minifyx.de/uwupulse/privacy/ |
| Support-URL | https://github.com/MinifyX/UwUPulse/issues |
| Marketing-URL | https://uwu.minifyx.de/#uwupulse |
| Copyright | 2026 MinifyX |
| Apple-Silicon-Mac / Vision Pro | aus (nur iPhone + Watch; am Mac gibt es die Desktop-App) |
| App-Datenschutz | „Keine Daten erfasst“ (nichts verlässt das lokale Netz, kein Entwickler-Server) |
| Exportkonformität | Verschlüsselung nur zwischen eigenen Geräten des Nutzers (X25519 und ChaCha20-Poly1305 aus Apples CryptoKit) → nach Kategorie 5 Teil 2, Note 3 (Massenmarkt) ausgenommen; `ITSAppUsesNonExemptEncryption = false` passt |
| Review-Hinweis | siehe unten |

## English

**Name** (≤30): `UwUPulse`
**Subtitle** (≤30): `Your heart rate live on stream`
**Promotional text** (≤170):
> Send your heart rate from Apple Watch or a Bluetooth chest strap straight to your computer: OBS overlay, recording and a local API. No cloud, no account.

**Keywords** (≤100):
`heart rate,bpm,obs,overlay,stream,twitch,pulse,watch,chest strap,polar,streamer,widget,local,hrm`

**Description:**
> UwUPulse brings your heart rate from your Apple Watch or a Bluetooth chest strap to your computer – live, in your own network, with no cloud and no account.
>
> Pair your iPhone with the free UwUPulse desktop app for macOS, Windows or Linux by scanning a QR code. Start a session on your Watch or connect a chest strap, and your BPM appears on the desktop within a second.
>
> FOR STREAMERS
> • Ready-made overlays for OBS and other streaming software as a browser source
> • Presets, an editor and custom CSS
> • Heart animation that beats with your pulse
>
> RECORD AND USE YOUR DATA
> • Record sessions with min, max and average on your computer
> • Export as CSV or JSON
> • Local API and WebSocket for your own scripts and bots
>
> PRIVATE BY DESIGN
> • Everything stays in your local network, end-to-end encrypted between your devices
> • No server, no account, no tracking, no ads
> • Nothing is saved to Apple Health
> • Open source (MIT): github.com/MinifyX/UwUPulse
>
> WORKS WITH
> • Apple Watch (watchOS 11 or later)
> • Bluetooth heart rate straps with the standard heart rate profile (e.g. Polar, Garmin, Wahoo)
>
> Requires the free UwUPulse desktop app on a computer in the same network.

**What's new (1.0.0):** `First release.`

## Deutsch

**Name:** `UwUPulse`
**Untertitel** (≤30): `Dein Puls, live im Stream`
**Werbetext** (≤170):
> Schick deine Herzfrequenz von der Apple Watch oder einem Bluetooth-Brustgurt direkt an deinen Computer: OBS-Overlay, Aufzeichnung und lokale API. Ohne Cloud, ohne Konto.

**Schlüsselwörter** (≤100):
`herzfrequenz,puls,bpm,obs,overlay,stream,twitch,watch,brustgurt,polar,streamer,widget,lokal`

**Beschreibung:**
> UwUPulse bringt deine Herzfrequenz von der Apple Watch oder einem Bluetooth-Brustgurt auf deinen Computer – live, im eigenen Netzwerk, ohne Cloud und ohne Konto.
>
> Koppel dein iPhone per QR-Code mit der kostenlosen UwUPulse-Desktop-App für macOS, Windows oder Linux. Starte eine Sitzung auf der Watch oder verbinde einen Brustgurt, und dein Puls erscheint in unter einer Sekunde auf dem Desktop.
>
> FÜR STREAMER
> • Fertige Overlays für OBS und andere Streaming-Software als Browserquelle
> • Vorlagen, Editor und eigenes CSS
> • Herz-Animation im Takt deines Pulses
>
> AUFZEICHNEN UND WEITERVERWENDEN
> • Sitzungen mit Minimum, Maximum und Durchschnitt auf dem Computer aufzeichnen
> • Export als CSV oder JSON
> • Lokale API und WebSocket für eigene Skripte und Bots
>
> PRIVAT VON GRUND AUF
> • Alles bleibt im lokalen Netzwerk, Ende-zu-Ende verschlüsselt zwischen deinen Geräten
> • Kein Server, kein Konto, kein Tracking, keine Werbung
> • Nichts wird in Apple Health gespeichert
> • Open Source (MIT): github.com/MinifyX/UwUPulse
>
> FUNKTIONIERT MIT
> • Apple Watch (ab watchOS 11)
> • Bluetooth-Brustgurten mit Standard-Herzfrequenzprofil (z. B. Polar, Garmin, Wahoo)
>
> Benötigt die kostenlose UwUPulse-Desktop-App auf einem Computer im selben Netzwerk.

**Neu (1.0.0):** `Erste Version.`

## Hinweise für das App Review (Englisch)

> UwUPulse forwards heart rate to the free companion desktop app (github.com/MinifyX/UwUPulse/releases) on the same local network; there is no server or account. To test without a computer: open the app, allow Health access, and start a session from the Apple Watch app or connect any Bluetooth heart rate strap — the live BPM is shown on the iPhone home screen. Pairing with a computer needs the desktop app, which shows a QR code under Devices → Pair phone. HealthKit is used only to run a workout session on the Watch for live heart rate; the workout is discarded and nothing is written to Health. Background Bluetooth keeps a connected chest strap streaming while the phone is locked.

## Screenshots

Pflichtgrößen: iPhone 6,9" (1320×2868 oder 1290×2796), Apple Watch (z. B. Ultra 3: 422×514). Aufnahme im Simulator mit Startargument `-demo` (nur DEBUG), Ablage `apple/build/screens/` (nicht im Repo).

## Antwort auf Guideline 2.1 (Information Needed, 2026-10-30)

Ins Feld „Notes“ der App-Review-Informationen (Englisch), zusammen mit der Bildschirmaufnahme von einem echten iPhone (iOS-Release-Version, mit gekoppelter Apple Watch):

> **Screen recording (physical iPhone, latest iOS, paired Apple Watch):** <Link/Anhang>. It starts with launching the app and shows: Health permission, starting a live session from the Watch, live BPM on the iPhone, connecting a Bluetooth chest strap, camera permission and scanning the pairing QR code from the desktop app, and the BPM arriving in the desktop overlay.
>
> **1. Purpose and audience.** UwUPulse shows your live heart rate where you need it: streamers get a heart rate overlay in OBS, athletes and gamers can record sessions and read the value over a local API. The iPhone/Watch app measures; the free desktop app (github.com/MinifyX/UwUPulse/releases, MIT) displays and stores it. It is a local alternative to cloud services.
>
> **2. Setup.** No login, no account, no in-app purchases. Without a computer: open the app, allow Health access, start a session on the Apple Watch (or connect any Bluetooth LE heart rate strap); live BPM appears on the home screen. With a computer: install the desktop app, open Devices → Pair phone to show a QR code, scan it in the iPhone app (or pair by comparing a 6-digit code), then the BPM shows up in the desktop app and the OBS overlay.
>
> **3. External services.** None. No server, no cloud, no analytics, no third-party SDKs. Data goes only from the Watch to the iPhone (WatchConnectivity) and from the iPhone to the user's own computer over the local network (encrypted WebSocket, Bonjour discovery). Apple frameworks: HealthKit (workout session on the Watch for live heart rate; nothing is written to Health), CoreBluetooth, AVFoundation (QR scan), Network.
>
> **4. Regional differences.** None; the app works the same in all regions. Localized in English and German.
>
> **5. Regulated industry / third-party material.** Not applicable. UwUPulse is not a medical device and makes no medical claims.
