# Data Profiles

## APD_Computer_Aided_Dispatch_Incidents_20251101.csv
- Estimated rows: **5,202,566** | Sampled: 200,000
- Candidate target: `Report Written Flag`
- Suggested feature columns (11): `Priority Level`, `Response Time`, `Number of Units Arrived`, `Unit Time on Scene`, `Mental Health Flag`, `Council District`, `Response Hour`, `Response Day of Week`, `Initial Problem Category`, `Final Problem Category`, `Call Disposition Description`

| Column | Role | Missing % | Unique | dtype | examples |
| --- | --- | --- | --- | --- | --- |
| Incident Number | metric | 0.0% | 199,998 | int64 | 232491149, 232671039, 231671034 |
| Incident Type | categorical | 0.0% | 2 | object | Officer-Initiated Incident, Dispatched Incident, Dispatched Incident |
| Council District | numeric | 0.0% | 11 | int64 | 7, 7, 7 |
| Mental Health Flag | categorical | 0.0% | 2 | object | Not Mental Health Incident, Not Mental Health Incident, Not Mental Health Incident |
| Priority Level | categorical | 0.0% | 4 | object | Priority 3, Priority 2, Priority 2 |
| Response Datetime | timestamp_string | 0.0% | 199,949 | object | 2023 Sep 06 05:46:10 PM, 2023 Sep 24 03:53:03 PM, 2023 Jun 16 04:44:39 PM |
| Response Year | numeric | 0.0% | 11 | int64 | 2023, 2023, 2023 |
| Response Month | categorical | 0.0% | 12 | object | Sep, Sep, Jun |
| Response Day of Week | categorical | 0.0% | 7 | object | Wed, Sun, Fri |
| Response Hour | numeric | 0.0% | 24 | int64 | 17, 15, 16 |
| First Unit Arrived Datetime | timestamp_string | 0.0% | 199,932 | object | 2023 Sep 06 05:46:10 PM, 2023 Sep 24 05:59:14 PM, 2023 Jun 16 04:49:12 PM |
| Call Closed Datetime | timestamp_string | 0.0% | 199,927 | object | 2023 Sep 06 06:06:34 PM, 2023 Sep 24 06:23:30 PM, 2023 Jun 16 04:58:25 PM |
| Sector | categorical | 0.0% | 11 | object | Edward, Edward, Edward |
| Initial Problem Description | text | 0.0% | 391 | object | Stalled Vehicle, Found/Abandoned Hazardous, Trespass Urgent |
| Initial Problem Category | categorical | 0.0% | 39 | object | Traffic Stop/Hazard, Drugs, Trespassing |
| Final Problem Description | text | 0.0% | 617 | object | Stalled Vehicle, Found/Abandoned Hazardous, Trespass Urgent |
| Final Problem Category | text | 0.0% | 40 | object | Traffic Stop/Hazard, Drugs, Trespassing |
| Number of Units Arrived | numeric | 0.0% | 53 | int64 | 2, 1, 1 |
| Unit Time on Scene | timestamp_string | 0.0% | 27,705 | object | 1,409, 1,456, 553 |
| Call Disposition Description | categorical | 0.0% | 21 | object | No Report, No Report, Unable To Locate |
| Report Written Flag | categorical | 0.0% | 2 | object | No, No, No |
| Response Time | timestamp_string | 30.5% | 11,901 | object | 7,718, 323, 1,563 |
| Officer Injured/Killed Count | metric | 0.0% | 1 | int64 | 0, 0, 0 |
| Subject Injured/Killed Count | metric | 0.0% | 2 | int64 | 0, 0, 0 |
| Other Injured/Killed Count | metric | 0.0% | 1 | int64 | 0, 0, 0 |
| Geo ID | identifier | 1.3% | 678 | float64 | 484530412002.0, 484530440002.0, 484530412001.0 |
| Census Block Group | numeric | 1.3% | 678 | float64 | 4530412002.0, 4530440002.0, 4530412001.0 |

### Top Categories (sample)
- **Incident Type:** Dispatched Incident (139089), Officer-Initiated Incident (60911)
- **Mental Health Flag:** Not Mental Health Incident (189431), Mental Health Incident (10569)
- **Priority Level:** Priority 3 (95684), Priority 2 (65950), Priority 1 (24231), Priority 0 (14135)
- **Response Datetime:** 2014 Nov 03 02:38:53 PM (2), 2021 Nov 16 06:07:27 AM (2), 2015 Jan 29 05:54:18 PM (2), 2016 Feb 24 02:41:04 PM (2), 2019 Jun 22 11:04:27 AM (2), 2019 Oct 14 05:17:58 PM (2), 2020 Jan 24 05:59:56 AM (2), 2016 Feb 16 08:34:19 AM (2)
- **Response Month:** Mar (17674), May (17539), Aug (17174), Jul (17083), Jun (17055), Oct (17004), Jan (16738), Apr (16559)
- **Response Day of Week:** Fri (30920), Thu (29341), Wed (28635), Tue (28456), Mon (28432), Sat (28052), Sun (26164)
- **First Unit Arrived Datetime:** 2018 Aug 28 03:04:53 PM (2), 2021 Nov 17 05:35:29 PM (2), 2018 Nov 04 02:50:45 PM (2), 2019 Nov 04 07:05:32 PM (2), 2018 Jan 19 07:22:01 PM (2), 2023 Mar 17 03:28:38 AM (2), 2017 Aug 26 11:02:36 AM (2), 2018 Jun 15 07:01:33 PM (2)
- **Call Closed Datetime:** 2024 Feb 26 07:37:59 PM (2), 2020 Feb 17 06:57:07 PM (2), 2018 Jan 21 08:43:46 PM (2), 2014 Mar 29 02:57:40 PM (2), 2016 May 12 12:33:45 AM (2), 2020 Jul 07 06:13:20 AM (2), 2017 May 19 09:38:48 PM (2), 2023 Aug 21 06:43:32 PM (2)
- **Sector:** Edward (25608), David (25179), Baker (23781), Adam (22578), Frank (21411), George (20493), Ida (19263), Henry (18392)
- **Initial Problem Description:** Traffic Stop (17702), Disturbance Other (12367), Alarm Burglar (11553), On Site Incident (11315), Check Welfare Urgent (9137), Trespass Urgent (8804), Suspicious Person (8793), Doc / C.o. Violation (7310)
- **Initial Problem Category:** Traffic Stop/Hazard (31070), Other (26038), Disturbance (22864), Administrative (21423), Welfare Check (14829), Suspicious Things (14697), Alarms (13281), Crashes (10884)
- **Final Problem Description:** Traffic Stop (15561), Disturbance Other (8063), Suspicious Person (7461), Doc / C.o. Violation (6950), False Burglar Alarm (6515), Trespass Urgent (6434), Traffic Hazard (5936), Checking Area (5670)
- **Final Problem Category:** Traffic Stop/Hazard (29072), Administrative (21781), Disturbance (18043), Other (15686), Welfare Check (13782), Alarms (12919), Assistance (12856), Suspicious Things (12513)
- **Unit Time on Scene:** 3 (297), 5 (277), 4 (270), 6 (235), 2 (234), 7 (220), 8 (183), 9 (175)
- **Call Disposition Description:** No Report (91744), Report Written (45895), Unable To Locate (21327), 10/8 From Traffic (18835), False Alarm (11843), Supplement Written (4642), Non-Police Matter (1996), Report Written MH (1448)
- **Report Written Flag:** No (153766), Yes (46234)
- **Response Time:** 1 (1172), 0 (1143), 366 (168), 395 (164), 325 (162), 276 (159), 336 (157), 331 (156)

## LAFD_Response_Metrics_-_Raw_Data_20251101.csv
- Estimated rows: **7,201,913** | Sampled: 200,000
- Suggested feature columns (9): `Emergency Dispatch Code`, `Dispatch Sequence`, `Dispatch Status`, `Unit Type`, `PPE Level`, `First In District`, `Time of Dispatch (GMT)`, `En Route Time (GMT)`, `On Scene Time (GMT)`

| Column | Role | Missing % | Unique | dtype | examples |
| --- | --- | --- | --- | --- | --- |
| Randomized Incident Number | text | 0.0% | 131,364 | object | 201,704,829,775, 201,703,779,048, 201,701,709,984 |
| First In District | numeric | 0.0% | 107 | float64 | 56.0, 102.0, 9.0 |
| Emergency Dispatch Code | categorical | 0.0% | 2 | object | Emergency, Emergency, Emergency |
| Dispatch Sequence | numeric | 0.0% | 81 | int64 | 1, 2, 1 |
| Dispatch Status | categorical | 0.0% | 3 | object | QTR, QTR, QTR |
| Unit Type | categorical | 0.0% | 5 | object | E - ENGINE, E - ENGINE, RA8XX - BLS RESCUE AMBULANCE |
| PPE Level | categorical | 0.0% | 2 | object | EMS, EMS, EMS |
| Incident Creation Time (GMT) | timestamp_string | 0.0% | 169,291 | object | 01:57:20.970192, 20:05:16.636014, 06:54:51.009957 |
| Time of Dispatch (GMT) | timestamp_string | 0.0% | 199,999 | object | 01:58:11.589469, 20:06:38.245310, 06:56:18.121419 |
| En Route Time (GMT) | timestamp_string | 2.4% | 195,247 | object | 01:58:52.703919, 20:07:19.991881, 06:59:19.062039 |
| On Scene Time (GMT) | timestamp_string | 15.1% | 169,768 | object | 02:03:19.480396, 20:10:11.191347, 07:03:11.292549 |

### Top Categories (sample)
- **Randomized Incident Number:** 201,704,638,493 (26), 201,702,859,000 (23), 201,703,589,279 (19), 201,704,738,991 (13), 201,701,749,423 (12), 201,701,768,524 (12), 201,702,609,736 (12), 201,704,678,769 (11)
- **Emergency Dispatch Code:** Emergency (191486), Non-Emergency (8514)
- **Dispatch Status:** QTR (163881), RAD (31560), OTHER (4559)
- **Unit Type:** E - ENGINE (69832), RA - ALS RESCUE AMBULANCE (66330), RA8XX - BLS RESCUE AMBULANCE (42175), T - TRUCK (21606), RA6XX - RESERVE RESCUE AMBULANCE (57)
- **PPE Level:** EMS (168142), Non-EMS (31858)
- **Incident Creation Time (GMT):** 12:52:09.084878 (26), 19:46:55.990303 (22), 20:26:04.349741 (19), 11:06:17.831198 (12), 20:01:29.642131 (12), 03:08:09.000637 (11), 01:04:42.000000 (11), 23:54:55.663160 (10)
- **Time of Dispatch (GMT):** 00:23:45.624054 (2), 10:07:03.994248 (1), 19:42:27.110977 (1), 05:42:46.978145 (1), 21:02:58.257372 (1), 18:59:46.829485 (1), 21:42:17.897784 (1), 06:17:06.245865 (1)
- **En Route Time (GMT):** 22:20:38.308226 (2), 10:08:28.522524 (1), 19:43:12.836463 (1), 05:43:18.829603 (1), 21:03:20.197900 (1), 18:59:51.344189 (1), 21:42:45.315123 (1), 06:17:53.979630 (1)
- **On Scene Time (GMT):** 19:31:23.432604 (1), 02:03:19.480396 (1), 20:10:11.191347 (1), 07:03:11.292549 (1), 17:06:20.721083 (1), 01:58:35.240255 (1), 05:29:14.558464 (1), 09:59:17.335621 (1)
