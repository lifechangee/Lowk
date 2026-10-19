# Доступ Mac → Windows ПК по SSH (для сборки и тестов Lowk)

Цель: Mac (MacBook Air M4) безопасно запускает команды на вашем Windows-ПК (i5-10400F, RTX 3060), чтобы собирать и тестировать Tauri/Rust-приложение.

Принципы: только вход по ключу (без пароля), только домашняя сеть, порт 22 в интернет не открываем, включаем SSH только когда нужно.

Все блоки ниже вставляются в **PowerShell от имени администратора** (Пуск → напишите `PowerShell` → правый клик → «Запуск от имени администратора»), если не сказано иное.

---

## Шаг 0. Какая у вас Windows

Нажмите `Win + R`, введите `winver`, Enter. Должно быть Windows 10 (версия 1809+) или Windows 11. Если версия старше, сначала обновите Windows. Сообщите разработчику, что там написано.

## Шаг 1. (Рекомендуется) Отдельный пользователь для сборок

Так, даже если ключ украдут, доступ будет к обычной учётке, а не к администратору.

```powershell
$p = Read-Host -AsSecureString "Придумайте пароль для lowkbuild"
New-LocalUser -Name "lowkbuild" -Password $p -PasswordNeverExpires -Description "Lowk SSH builds"
# в группу Administrators НЕ добавляем - это обычный пользователь
```

Последствия: установку инструментов (Visual Studio Build Tools, Rust, Node, WebView2) нужно сделать **один раз** под вашей админской учёткой, потом они доступны всем. Затем зайдите под `lowkbuild` хотя бы раз (или `runas`), чтобы создалась папка профиля `C:\Users\lowkbuild`. Если это неудобно, можно использовать свою учётку, но тогда см. «ловушку админов» в шаге 4.

## Шаг 2. Установить OpenSSH Server

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
```

(Либо: Параметры → Приложения → Дополнительные компоненты → Добавить компонент → «Сервер OpenSSH».)

**Запуск службы: вручную или автоматически?**
- **Вручную (рекомендуется):** порт закрыт, пока вы сами не включили. Минус: перед работой нужно выполнить одну команду.
- **Автоматически:** удобно, но SSH слушает сеть постоянно (только в локальной сети, но всё же).

```powershell
# вручную (рекомендуется)
Set-Service -Name sshd -StartupType Manual
# запустить, когда нужно:
Start-Service sshd
# остановить после работы:
Stop-Service sshd
```

Для автоматического запуска: `Set-Service -Name sshd -StartupType Automatic`.

Выполните разово `Start-Service sshd` — при первом старте создастся `C:\ProgramData\ssh\`.

## Шаг 3. Только ключи, без паролей

Откройте конфиг:

```powershell
notepad C:\ProgramData\ssh\sshd_config
```

Найдите (или добавьте) строки, **без `#` в начале**:

```
PasswordAuthentication no
PubkeyAuthentication yes
PermitEmptyPasswords no
AllowUsers lowkbuild
```

(`AllowUsers` — впишите ваше имя, если не создавали `lowkbuild`.) В самом конце файла **закомментируйте** (поставьте `#`) блок `Match Group administrators` с двумя строками под ним, только если используете обычного пользователя `lowkbuild`. Сохраните и перезапустите:

```powershell
Restart-Service sshd
```

## Шаг 4. Вставить публичный ключ Mac

Публичный ключ (его можно показывать, он безопасен; приватный остаётся только на Mac):

```
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKXJmib4rEb2C3ok3HGQNbCeqewmiMz0PJRDwEiRmuZX lowk-dev-mac
```

**Вариант А: обычный пользователь `lowkbuild`** (PowerShell, можно без админа, но от имени этого пользователя; проще из админского окна, как ниже):

```powershell
$dir = "C:\Users\lowkbuild\.ssh"
New-Item -ItemType Directory -Force $dir | Out-Null
Set-Content -Path "$dir\authorized_keys" -Value 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKXJmib4rEb2C3ok3HGQNbCeqewmiMz0PJRDwEiRmuZX lowk-dev-mac' -Encoding ascii
icacls $dir /inheritance:r /grant "lowkbuild:(OI)(CI)F" "SYSTEM:(OI)(CI)F" "Administrators:(OI)(CI)F"
```

**Вариант Б: ловушка админов.** Если пользователь входит в группу Administrators, Windows игнорирует `~\.ssh\authorized_keys` и читает **`C:\ProgramData\ssh\administrators_authorized_keys`**, причём с жёсткими правами:

```powershell
$f = "C:\ProgramData\ssh\administrators_authorized_keys"
Set-Content -Path $f -Value 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKXJmib4rEb2C3ok3HGQNbCeqewmiMz0PJRDwEiRmuZX lowk-dev-mac' -Encoding ascii
icacls $f /inheritance:r /grant "Administrators:F" "SYSTEM:F"
```

Если права не такие, ключ молча не работает.

## Шаг 5. Файрвол: только домашняя сеть и только Mac

1. Убедитесь, что домашняя сеть имеет профиль **Частная**: Параметры → Сеть и Интернет → ваше подключение → Тип сетевого профиля → «Частная».
2. Узнайте IP Mac: на Mac `Системные настройки → Сеть → Wi-Fi → Подробнее` (или в Терминале `ipconfig getifaddr en0`). Дальше подставьте его вместо `MAC_IP`.

```powershell
# убрать автоматическое общее правило (оно открывает SSH на все профили)
Disable-NetFirewallRule -Name "OpenSSH-Server-In-TCP" -ErrorAction SilentlyContinue
# своё правило: только Private, только с Mac
New-NetFirewallRule -Name "Lowk-SSH-LAN" -DisplayName "Lowk SSH (LAN, Mac only)" `
  -Direction Inbound -Protocol TCP -LocalPort 22 -Action Allow `
  -Profile Private -RemoteAddress MAC_IP
```

Если IP Mac меняется, закрепите и его в роутере (см. ниже) или временно используйте `-RemoteAddress LocalSubnet`.

## Шаг 6. IP-адрес ПК и его закрепление

Узнать адрес ПК:

```powershell
Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -like "192.168.*" -or $_.IPAddress -like "10.*" } | Select IPAddress, InterfaceAlias
```

Нужен адрес вида `192.168.1.xx`. Чтобы он не менялся, сделайте **DHCP-резервирование** (простыми словами: роутер всегда выдаёт этому ПК один и тот же адрес):
1. Откройте админку роутера (обычно `192.168.1.1` или `192.168.0.1` в браузере, логин/пароль на наклейке роутера).
2. Найдите раздел «DHCP» → «Резервирование адресов» / «Привязка по MAC» / «Static lease».
3. Выберите ваш ПК в списке устройств и нажмите «Закрепить / Reserve». Сохраните.

**Никогда не делайте «проброс портов» (port forwarding) для порта 22 и не включайте «DMZ».** Это открыло бы ПК всему интернету.

Пришлите разработчику: IP ПК и имя пользователя. Он впишет их в `~/.ssh/config` на Mac (там уже есть шаблон `Host lowk-win`).

## Шаг 7. Проверка (на Mac)

После вписывания IP и имени в `~/.ssh/config`:

```bash
ssh lowk-win "hostname && whoami"
```

Должно вывести имя ПК и пользователя без запроса пароля. Если спрашивает пароль, значит ключ не подхвачен (см. шаг 4 и права icacls).

---

## GUI-тесты (оверлей поверх Zoom и т.п.)

Процесс, запущенный через SSH, живёт в отдельной «сессии» и **не может показывать окна на вашем рабочем столе**. Обходные пути:

**Вариант 1. Запланированная задача в вашей сессии** (разовая настройка, пока вы залогинены на ПК):

```powershell
$a = New-ScheduledTaskAction -Execute "C:\Lowk\run-gui-test.bat"
$p = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive
Register-ScheduledTask -TaskName "LowkGuiTest" -Action $a -Principal $p
```

Затем Mac запускает: `ssh lowk-win "schtasks /run /tn LowkGuiTest"` — окно появится на вашем экране (это эквивалент `schtasks /IT`). Условие: вы залогинены и экран не заблокирован.

**Вариант 2. Проще:** положите `run-gui-test.bat` на рабочий стол и дважды кликните его, когда разработчик попросит. Результаты он заберёт по SSH из лог-файла.

---

## Доступ вне дома (необязательно): Tailscale

Бесплатно, без открытых портов на роутере: устройства соединяются через зашифрованную частную сеть.

1. Установите Tailscale на ПК и на Mac (tailscale.com/download), войдите в один и тот же аккаунт.
2. В файрволе добавьте правило для адреса Tailscale (`100.64.0.0/10`) вместо домашнего IP:
   ```powershell
   New-NetFirewallRule -Name "Lowk-SSH-Tailscale" -DisplayName "Lowk SSH (Tailscale)" `
     -Direction Inbound -Protocol TCP -LocalPort 22 -Action Allow -Profile Any -RemoteAddress 100.64.0.0/10
   ```
3. В `~/.ssh/config` на Mac поменяйте `HostName` на Tailscale-адрес ПК (`100.x.y.z`) или его имя в Tailscale.
4. Ограничьте доступ: в админке Tailscale (Access controls) разрешите на ПК только порт 22 и только с вашего Mac, остальным устройствам запретите. Не включайте «Funnel» и «subnet routes» для этого ПК.
5. Tailscale SSH (встроенный) на Windows не поддерживается, поэтому используем обычный OpenSSH с ключом, как выше.

---

## Как всё выключить / отозвать доступ

Быстро (после каждой сессии):

```powershell
Stop-Service sshd
```

Полностью отозвать доступ Mac:

```powershell
Stop-Service sshd
Set-Service sshd -StartupType Disabled
Disable-NetFirewallRule -Name "Lowk-SSH-LAN" -ErrorAction SilentlyContinue
Disable-NetFirewallRule -Name "Lowk-SSH-Tailscale" -ErrorAction SilentlyContinue
# удалить ключ (выберите свой вариант):
Remove-Item "C:\Users\lowkbuild\.ssh\authorized_keys" -ErrorAction SilentlyContinue
Remove-Item "C:\ProgramData\ssh\administrators_authorized_keys" -ErrorAction SilentlyContinue
# удалить пользователя для сборок и сам сервер:
Remove-LocalUser -Name "lowkbuild"
Remove-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Unregister-ScheduledTask -TaskName "LowkGuiTest" -Confirm:$false -ErrorAction SilentlyContinue
```

Если ключ могли скомпрометировать: сделайте шаги выше, затем на Mac удалите `~/.ssh/lowk_windows*` и сгенерируйте новый ключ.
