#!/usr/bin/env python3
"""Generates fixtures/index/eval.json (RU/EN/code-switch retrieval eval). Run from anywhere:
   python3 tools/index-bench/gen_eval.py
Every topic: one passage and three queries: kw (shares content words), para (paraphrase with no
shared content words: needs semantics), xl (other language than the passage, English terms kept
as spoken in code-switched calls)."""
import json, pathlib

# (kind, title, text, kw, para, xl)
T = [
("call","Поставка: оплата", "Клиент просит разбить оплату по договору поставки на три транша. Первый транш аванс тридцать процентов сразу после подписания, остальное после приёмки оборудования на складе.",
 "аванс тридцать процентов три транша", "заказчик хочет платить частями, а не вносить всю сумму заранее", "payment installments upfront deposit"),
("call","DB migration", "We agreed to migrate the database from Postgres 12 to Postgres 16 over the weekend. If replication lag exceeds ten minutes we roll back to the old primary and reschedule.",
 "Postgres migration rollback replication lag", "moving our data store to a newer release during days off with a way back", "миграция базы данных откат на старую версию"),
("call","Найм дизайнера", "Обсуждали найм продуктового дизайнера. Кандидат Анна показала портфолио, зарплатные ожидания двести тысяч рублей, выйти может через месяц, нужен тестовый день в команде.",
 "найм дизайнера Анна зарплатные ожидания", "рассматриваем человека на позицию по интерфейсам и сколько он хочет получать", "hiring a product designer salary expectations"),
("doc","API limits", "# Rate limits\n\nThe public API allows 100 requests per minute per key. Exceeding the quota returns HTTP 429 with a Retry-After header. Burst traffic up to 20 requests is tolerated.",
 "rate limit 100 requests per minute 429", "what happens when a client sends too many calls in a short time", "лимит запросов в минуту ошибка 429"),
("call","Релиз 4.2", "По релизу четыре точка два: фича-флаг для нового онбординга включаем на десять процентов пользователей, если конверсия не падает, к пятнице раскатываем на всех.",
 "релиз 4.2 фича-флаг онбординг десять процентов", "постепенно включаем новую функцию для малой части аудитории и следим за метриками", "feature flag gradual rollout onboarding"),
("call","Budget review", "The marketing budget for Q4 is cut by fifteen percent. We keep paid search, pause the podcast sponsorship and move the saved money into the customer referral programme.",
 "marketing budget Q4 cut paid search referral", "spending on promotion drops, some channels stop, money goes to word of mouth", "бюджет на маркетинг сокращён реферальная программа"),
("doc","Политика отпусков", "# Отпуска\n\nСотрудник вправе взять двадцать восемь календарных дней отпуска в год. Заявление подаётся руководителю не позднее чем за две недели. Неиспользованные дни переносятся не более чем на год.",
 "отпуск двадцать восемь календарных дней заявление", "сколько выходных в год положено работнику и как их оформить", "vacation policy days per year request"),
("call","Security incident", "Post-mortem of the outage: an expired TLS certificate on the payment gateway caused forty minutes of failed checkouts. Action item: automatic renewal and an alert fourteen days before expiry.",
 "outage expired TLS certificate payment gateway renewal alert", "checkout stopped working because a security credential ran out and nobody was warned", "инцидент просроченный сертификат платёжный шлюз"),
("call","Переговоры по цене", "Партнёр просит скидку пятнадцать процентов за годовой контракт. Мы готовы на десять, но только при предоплате за весь год. Решили вернуться к вопросу в четверг.",
 "скидка пятнадцать процентов годовой контракт предоплата", "контрагент торгуется за более низкую цену при долгом сотрудничестве", "discount for annual contract prepayment negotiation"),
("doc","Onboarding checklist", "# New hire checklist\n\nOn day one the employee receives a laptop, access to email and the VPN, and a buddy from the team. By the end of week one they complete security training and ship a small pull request.",
 "new hire checklist laptop VPN buddy security training", "what a freshly joined person gets and must finish during the first days at the company", "чек-лист для нового сотрудника доступы наставник"),
("call","Обсуждение метрик", "Смотрели удержание: когортный анализ показал, что на тридцатый день возвращается только двадцать два процента пользователей. Гипотеза: пуш-уведомления приходят слишком поздно.",
 "удержание когортный анализ тридцатый день двадцать два процента", "сколько людей продолжает пользоваться продуктом через месяц и почему они уходят", "retention cohort analysis day 30 push notifications"),
("call","Sprint planning", "Sprint planning: the team commits to eight stories, the payments refactor is the biggest risk, estimated at thirteen points. Carlos will pair with Dmitry on the webhook retries.",
 "sprint planning eight stories payments refactor thirteen points webhook retries", "the group picks its workload for the next two weeks and flags the riskiest item", "планирование спринта рефакторинг платежей"),
("doc","Договор: ответственность", "# Ответственность сторон\n\nЗа просрочку поставки поставщик уплачивает неустойку в размере 0,1 процента от стоимости товара за каждый день, но не более десяти процентов общей суммы.",
 "неустойка 0,1 процента просрочка поставки", "штраф, который платит продавец, если опоздал с отгрузкой", "late delivery penalty percent per day contract"),
("call","Кандидат: интервью", "Интервью с бэкенд-разработчиком Иваном: уверенно знает Go и Kubernetes, слабее в базах данных. Задачу на проектирование очереди решил с подсказками. Рекомендация: брать на middle.",
 "интервью бэкенд Go Kubernetes middle", "оценка собеседования программиста сильные и слабые стороны", "backend engineer interview feedback Go Kubernetes"),
("call","Customer complaint", "The customer reported that invoices are issued in the wrong currency. Support confirmed that the account was created with euros but billing defaulted to dollars. We will refund the difference and fix the default.",
 "invoices wrong currency euros dollars refund", "a client was charged in a different money than they signed up with and gets money back", "жалоба клиента счёт в неправильной валюте возврат"),
("doc","Backup policy", "# Backups\n\nFull backups run nightly at 02:00 and are kept for thirty days. Incremental snapshots every hour are kept for 48 hours. Restore tests are performed on the first Monday of each month.",
 "backups nightly 02:00 thirty days restore tests", "how often copies of the data are made and how long they are retained", "резервное копирование ночью хранение тридцать дней"),
("call","Запуск рекламы", "Договорились запускать рекламу в Яндексе и на VK. Стартовый бюджет сто тысяч, цель стоимость лида не выше пятисот рублей. Креативы подготовит Лена до среды.",
 "реклама Яндекс VK бюджет сто тысяч стоимость лида", "платная раскрутка на двух площадках, лимит расходов и допустимая цена привлечения", "ad campaign launch budget cost per lead"),
("call","Roadmap Q1", "For Q1 the roadmap has three themes: self-serve billing, an Android app and SSO for enterprise. SSO slips to Q2 if the security review is not finished by the end of January.",
 "roadmap Q1 self-serve billing Android SSO enterprise", "plan for the first three months: payments by customers themselves, mobile client, corporate login", "роадмап на квартал SSO отложим"),
("doc","Инструкция по деплою", "# Деплой\n\nПеред выкладкой запустить миграции и прогнать смоук-тесты на стейджинге. Выкладка идёт через blue-green, откат выполняется переключением балансировщика на предыдущую версию.",
 "деплой миграции смоук-тесты стейджинг blue-green откат", "как выкатывать новую версию на боевые серверы безопасно и вернуть всё назад", "deployment runbook blue green rollback load balancer"),
("call","Привлечение инвестиций", "Разговор с инвестором: он готов вложить два миллиона долларов при оценке двенадцать миллионов до денег. Просит место в совете директоров и право на пропорциональное участие в следующих раундах.",
 "инвестор два миллиона долларов оценка совет директоров", "вкладчик предлагает деньги за долю и хочет влиять на управление", "investor term sheet valuation board seat pro rata"),
("call","Capacity planning", "Traffic doubled after the launch. The cluster runs at seventy percent CPU, so we add four nodes before the holiday sale. Autoscaling max is raised from twenty to thirty replicas.",
 "traffic doubled CPU seventy percent add nodes autoscaling replicas", "usage jumped so we need more servers before the busy shopping period", "нагрузка выросла добавляем ноды автомасштабирование"),
("doc","Пользовательская лицензия", "# Лицензия\n\nПродукт предоставляется на условиях неисключительной лицензии сроком на один год. Передача прав третьим лицам и декомпиляция запрещены. Техническая поддержка включена в стоимость.",
 "неисключительная лицензия один год декомпиляция запрещена", "разрешение пользоваться программой на ограниченный срок без права перепродавать", "software license one year no reverse engineering support included"),
("call","Ретро", "На ретроспективе отметили, что код-ревью занимает слишком много времени: пулреквесты висят по три дня. Решили ввести правило: ответ на ревью в течение четырёх часов и ограничить размер изменений.",
 "ретроспектива код-ревью пулреквесты четыре часа", "команда жалуется что проверка чужих изменений тормозит работу и вводит дедлайны на ответ", "retro code review pull requests slow turnaround"),
("call","Vendor selection", "We compared three vendors for transactional email. Vendor B is cheapest but has a poor deliverability record, so we choose Vendor A and keep B as a fallback for bulk campaigns.",
 "vendors transactional email deliverability fallback bulk campaigns", "choosing which company will send our automatic messages, balancing price against reliability", "выбор поставщика рассылок доставляемость"),
("call","Звонок с юристом", "Юрист пояснил, что персональные данные граждан России должны храниться на серверах в России. Нужно уведомить Роскомнадзор и обновить политику конфиденциальности до конца месяца.",
 "персональные данные серверы Россия Роскомнадзор политика конфиденциальности", "где по закону держать сведения о клиентах и какое ведомство надо известить", "data localization law notify regulator privacy policy"),
("doc","Pricing page copy", "# Pricing\n\nStarter costs nine dollars per user per month. Team costs nineteen dollars and adds shared folders and audit logs. Enterprise is custom and includes SSO and a dedicated manager.",
 "pricing Starter nine dollars Team nineteen audit logs Enterprise", "how much each subscription level costs and what extras the higher ones include", "тарифы стоимость за пользователя в месяц"),
("call","Планёрка по багам", "Критичный баг: при загрузке файла больше двухсот мегабайт приложение зависает. Воспроизводится только в Safari. Назначили Сергея, срок исправления до пятницы, затронуто около трёх процентов пользователей.",
 "баг загрузка файла двести мегабайт Safari зависает", "программа замирает на большом вложении, но только в одном браузере от Apple", "critical bug large file upload freezes Safari"),
("call","Partnership call", "The partner wants co-marketing: a joint webinar in March and a case study. In return they ask for a listing in our integrations marketplace with a revenue share of twenty percent.",
 "co-marketing joint webinar case study marketplace revenue share twenty percent", "working together on promotion and in exchange getting a cut of sales through our app store", "партнёрство совместный вебинар доля от выручки"),
("doc","Политика паролей", "# Пароли\n\nМинимальная длина пароля четырнадцать символов. Обязательна двухфакторная аутентификация для всех сотрудников. Пароли не хранятся в открытом виде, используется менеджер паролей компании.",
 "пароль минимальная длина четырнадцать двухфакторная аутентификация", "какие требования к секретным словам для входа и дополнительная проверка личности", "password policy minimum length two-factor authentication"),
("call","Customer success", "Churn risk: the account Northwind has not logged in for six weeks. CSM Maria will schedule a business review and offer a free training session on the reporting module.",
 "churn risk Northwind not logged in business review training reporting module", "an important customer went quiet so someone will meet them and offer help to keep them", "риск оттока клиент не заходит обучение"),
("call","Стратегия цен", "Обсудили повышение цены подписки с девятисот до тысячи двухсот рублей. Старые клиенты сохраняют тариф ещё полгода. Ожидаем отток около пяти процентов, но рост выручки на двадцать.",
 "повышение цены подписки тысяча двести рублей старые клиенты отток", "подорожание сервиса с бонусом для текущих покупателей и ожидаемые потери", "subscription price increase grandfathering churn revenue"),
("call","Конференция", "Готовимся к конференции в Казани: стенд шесть квадратных метров, три сотрудника едут от нас, раздаём стикеры и проводим живое демо в двенадцать и в четыре часа.",
 "конференция Казань стенд демо стикеры", "выставка где мы будем показывать продукт на небольшой площадке и раздавать сувениры", "conference booth demo schedule giveaways"),
("call","Склад и логистика", "На складе не хватает паллетных мест, поэтому часть товара везём напрямую от производителя к клиенту. Перевозчик берёт на двенадцать процентов дороже, но срок сокращается до двух дней.",
 "склад паллетные места перевозчик двенадцать процентов", "не хватает мест для хранения коробок, отгружаем минуя наш цех, платим больше за скорость", "warehouse capacity direct shipping carrier surcharge"),
("doc","Incident severity levels", "# Severity\n\nSev1 means customers cannot use the product at all and requires a response within fifteen minutes. Sev2 is a major degradation with a one hour response. Sev3 is a minor issue handled next business day.",
 "Sev1 Sev2 Sev3 response fifteen minutes", "how urgent a failure is and how quickly the on-call person must react", "уровни критичности инцидентов время реакции"),
]

# Passages that match nothing: hard negatives sharing vocabulary with the topics.
D = [
("call","Обед", "Обсуждали, куда пойти на обед: итальянский ресторан рядом с офисом или столовая на первом этаже. Решили в итальянский, бронь на час дня на шесть человек."),
("call","Small talk", "We chatted about the weekend: a short hike near the lake, the weather was great, and somebody's kid finished the marathon. Nothing work related came up."),
("doc","Офисные правила", "# Офис\n\nПереговорные бронируются в календаре. Еду в переговорных не оставлять. Последний ушедший выключает свет и закрывает окна."),
("doc","Glossary", "# Glossary\n\nARR is annual recurring revenue. MRR is monthly recurring revenue. NPS is net promoter score. CAC is customer acquisition cost."),
("call","Тест микрофона", "Раз, два, три, проверка связи. Слышно ли меня? Хорошо, давайте начнём через минуту, ждём ещё двоих участников."),
("call","Hardware", "The new laptops arrive on Thursday. IT will image them and hand them out. Old machines are collected and wiped before they are donated."),
("doc","Дизайн-система", "# Кнопки\n\nОсновная кнопка синяя, скругление восемь пикселей, высота сорок четыре. Вторичная кнопка с обводкой. В неактивном состоянии прозрачность пятьдесят процентов."),
("doc","Style guide", "# Writing style\n\nUse sentence case for headings. Avoid jargon. Prefer short sentences. Numbers above ten are written as digits."),
("call","Календарь", "Перенесли еженедельную встречу с вторника на среду, потому что у двоих коллег пересекается другая встреча. Остальное без изменений."),
("call","Parking", "Parking passes for the new office are limited to twenty spots. Employees who cycle get a locker instead. The list will be shared next week."),
("doc","Книжный клуб", "# Книжный клуб\n\nВ этом месяце читаем про привычки и продуктивность. Встреча в последнюю пятницу, приносите заметки и по одной цитате."),
("call","Training", "The all-hands training on public speaking was moved to the large room. Slides will be shared afterwards and attendance is optional."),
]

docs, queries = [], []
n = 0
for kind, title, text, kw, para, xl in T:
    n += 1
    did = f"t{n:02d}"
    docs.append({"id": did, "kind": kind, "title": title, "md": text if kind == "doc" else "## Transcript\n\n[00:00:05] **System:** " + text})
    for typ, q in (("kw", kw), ("para", para), ("xl", xl)):
        queries.append({"q": q, "rel": did, "type": typ})
for kind, title, text in D:
    n += 1
    docs.append({"id": f"n{n:02d}", "kind": kind, "title": title, "md": text if kind == "doc" else "## Transcript\n\n[00:00:05] **System:** " + text})

out = pathlib.Path(__file__).resolve().parents[2] / "fixtures/index/eval.json"
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps({"docs": docs, "queries": queries}, ensure_ascii=False, indent=1))
print(len(docs), "docs", len(queries), "queries ->", out)
