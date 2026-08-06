# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 20 of 63 — Phase 1: Foundations (Module 20 of 28)
**Module:** Cloud & AWS Basics
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Almost every system you design in Phase 2 will be deployed on a cloud, and AWS holds roughly a third of that market — it is the default lingua franca of system design interviews. When an interviewer says "put it behind a load balancer in a private subnet with a NAT gateway for egress," you must picture the exact boxes, not nod blankly. Today we build the mental model of the four primitives you'll assemble in every design: compute (EC2), the firewall (security groups), observability (CloudWatch), and the network fabric (VPC, subnets, gateways). Get these right and cloud architecture stops being jargon and becomes Lego.

## Learning Objectives
By the end of this lesson you can:
- Explain what an EC2 instance is, how instance types are named, and pick one by workload.
- Configure security groups as a stateful firewall and contrast them with NACLs.
- Use CloudWatch metrics, alarms, and logs to detect and react to problems.
- Draw a VPC with public and private subnets across availability zones.
- Explain the difference between an internet gateway and a NAT gateway and when each is used.

## The Lesson

### EC2 Instances
**What it is (plain English):** EC2 (Elastic Compute Cloud) rents you virtual servers by the second. You pick an OS image (AMI), a size, launch it, and get a Linux/Windows machine you SSH into — the cloud equivalent of a computer in a data center, but spun up in ~60 seconds.

**The problem it solves:** Owning hardware means weeks of procurement, fixed capacity, and idle waste. EC2 turns servers into an on-demand utility: scale from 1 to 1,000 machines in minutes and pay only while they run.

**How it works (mechanics):** A physical host runs a hypervisor (AWS's Nitro) that slices it into isolated VMs. Instance types are named `family.size`, e.g., `m5.xlarge`: family **m** = general purpose, **5** = generation, **xlarge** = 4 vCPU / 16 GB RAM. Families target workloads:
```
t3  - burstable, cheap (dev, low traffic)
m5  - general purpose (balanced CPU:RAM)
c5  - compute-optimized (CPU-heavy, e.g., encoding)
r5  - memory-optimized (caches, in-memory DBs)
```
Pricing models: **On-Demand** (flexible, ~$0.096/hr for m5.large), **Reserved/Savings Plans** (commit 1–3 yr, save up to ~72%), and **Spot** (spare capacity up to ~90% off, but can be reclaimed with a 2-minute warning).

**Trade-offs / when NOT to use it:** You manage the OS, patching, and scaling — that's toil. For simple request/response code, serverless (Lambda) or containers (ECS/Fargate) remove the server management. Spot is a trap for stateful, non-interruptible work.

**Where you'll see it:** The backbone of most AWS deployments — web tiers, app servers, databases, Kubernetes nodes (EKS). Netflix, Airbnb, and Lyft run enormous EC2 fleets.

### Firewall & Security Groups
**What it is (plain English):** A **security group (SG)** is a virtual firewall attached to an instance's network interface. It's an allow-list: you list which traffic is permitted; everything else is denied by default. There are no "deny" rules — absence of an allow *is* the deny.

**The problem it solves:** An instance directly exposed to the internet is attacked within minutes. SGs restrict who can reach which port, shrinking the attack surface to only what you explicitly open.

**How it works (mechanics):** SGs are **stateful**: if you allow an inbound request, the response is automatically allowed back out — you don't write a matching outbound rule. A typical web server:
```
INBOUND:
  TCP 443  from 0.0.0.0/0        (HTTPS from anyone)
  TCP 22   from 10.0.0.0/16      (SSH only from inside the VPC)
OUTBOUND:
  all allowed (default)
```
Best practice is chaining: the ALB's SG allows 443 from the world; the app-server SG allows 8080 *only from the ALB's SG*; the DB SG allows 5432 *only from the app SG*. Rules reference other SGs, not IPs — so it scales as instances come and go.

**Trade-offs / when NOT to use it:** SGs operate per-instance and can't express "deny this bad IP" — for subnet-wide deny rules you use **Network ACLs** (stateless, evaluated in order). SGs alone don't stop application-layer attacks (SQL injection); you add a WAF for that.

**Where you'll see it:** Every EC2, RDS, and ELB deployment. Security groups are the number-one thing misconfigured in AWS breaches (an SG open to `0.0.0.0/0` on port 22 or 3306 is a classic leak).

### CloudWatch
**What it is (plain English):** CloudWatch is AWS's built-in monitoring service. It collects **metrics** (numbers over time like CPU %), **logs** (text output from your apps), and lets you set **alarms** that fire when a metric crosses a threshold — the eyes and ears of your system.

**The problem it solves:** Flying blind. Without monitoring you learn about an outage from angry users, not from your own dashboards. CloudWatch turns "is it healthy?" into a measurable, alertable question.

**How it works (mechanics):** EC2 publishes basic metrics every 5 minutes free, or every 1 minute with detailed monitoring. You define an alarm, e.g.:
```
Alarm: CPUUtilization > 70% for 3 consecutive 1-min periods
   -> action 1: SNS notify on-call (PagerDuty/email)
   -> action 2: trigger Auto Scaling to add 2 instances
```
Logs flow via the CloudWatch agent; you query them with **Logs Insights**. Custom metrics let you track business signals (orders/min). Alarms have states: OK, ALARM, INSUFFICIENT_DATA — and drive automated remediation, not just paging.

**Trade-offs / when NOT to use it:** High-cardinality custom metrics and verbose logs get expensive fast (per-metric and per-GB-ingested pricing). CloudWatch is broad but shallow on distributed tracing — teams often add X-Ray, Datadog, or Prometheus/Grafana for richer, cheaper, or cross-cloud observability.

**Where you'll see it:** Default monitoring for every AWS service; the trigger source for Auto Scaling and automated incident response across essentially all AWS shops.

### VPC, Subnets, Internet & NAT Gateways
**What it is (plain English):** A **VPC** (Virtual Private Cloud) is your own isolated private network inside AWS — your slice of the cloud with a private IP range you control. **Subnets** carve that range into smaller blocks, each living in one Availability Zone (a distinct data center). Gateways are the doors in and out of the VPC.

**The problem it solves:** Isolation and controlled connectivity. You don't want your database reachable from the public internet, but you do want your web tier reachable. Subnets + gateways let you place each component at the right exposure level.

**How it works (mechanics):** You pick a CIDR block, e.g., `10.0.0.0/16` (65,536 addresses), then split it:
```
VPC 10.0.0.0/16
├── AZ-a
│   ├── Public  subnet 10.0.1.0/24  -> ALB, NAT GW   (route to IGW)
│   └── Private subnet 10.0.2.0/24  -> app servers, DB (route to NAT)
└── AZ-b   (mirror, for high availability)
    ├── Public  10.0.3.0/24
    └── Private 10.0.4.0/24
```
A subnet is "public" if its **route table** sends `0.0.0.0/0` to an **Internet Gateway (IGW)** — a two-way door giving instances public IPs and inbound reachability. Private subnets have no IGW route, so they're unreachable from outside. But private instances still need *outbound* internet (to download patches). Enter the **NAT Gateway**: it lives in a public subnet, and private subnets route `0.0.0.0/0` to it. The NAT lets traffic go *out and back* but blocks unsolicited *inbound* — so your DB can pull updates yet stay unreachable.
```
Private app -> NAT GW (public subnet) -> IGW -> internet   (outbound OK)
Internet -> IGW -> ??? -> private app                       (blocked)
```

**Trade-offs / when NOT to use it:** NAT Gateways cost money both per-hour (~$0.045/hr) and per-GB processed (~$0.045/GB) — high-egress workloads run up real bills, and teams use VPC endpoints to reach AWS services without NAT. Spreading subnets across ≥2 AZs is mandatory for HA; a single-AZ design dies with that data center.

**Where you'll see it:** Every non-trivial AWS deployment. The canonical pattern — public subnet for the load balancer, private subnets for app servers and databases, NAT for egress, across 2–3 AZs — is what interviewers expect you to draw.

## Comparison Table

| Dimension | Internet Gateway (IGW) | NAT Gateway |
|---|---|---|
| Direction | Inbound + outbound | Outbound only |
| Attached to | The VPC | A public subnet |
| Used by | Public subnets | Private subnets (for egress) |
| Gives public IP? | Yes (instances get one) | No (hides private IPs) |
| Typical resident | ALB, bastion host | (path for) app servers, DBs |
| Cost | Free | ~$0.045/hr + ~$0.045/GB |

**Verdict:** IGW is the front door for things that must be reachable; NAT is a one-way exit so private things can fetch updates without being exposed.

## Common Misconceptions
- **Myth:** Security groups need explicit outbound rules for responses. → **Reality:** They're stateful — allowed inbound traffic's response is auto-allowed out.
- **Myth:** A private subnet has no internet at all. → **Reality:** It has no *inbound* internet, but a NAT gateway gives it *outbound* access.
- **Myth:** Security groups can block a specific malicious IP. → **Reality:** SGs are allow-only; use Network ACLs (stateless, with deny rules) for that.
- **Myth:** One Availability Zone is fine. → **Reality:** AZs fail; production spreads across ≥2 AZs or an entire tier goes dark.
- **Myth:** CloudWatch gives per-second, per-instance visibility for free. → **Reality:** Basic metrics are 5-minute; 1-minute detailed monitoring and custom metrics cost extra.

## Real-World Case
In 2019 Capital One suffered a breach exposing ~100 million customer records. A misconfigured web application firewall was tricked (an SSRF attack) into requesting AWS instance metadata, retrieving temporary credentials, and then reading an S3 bucket. The lesson threads through today's topics: defense in depth matters — tight **security groups** and least-privilege roles limit what a compromised instance can reach, **private subnets** keep sensitive stores off the public internet, and **CloudWatch/CloudTrail** logging is how you *detect* anomalous access before it becomes 100M records. No single control is enough; the breach was a chain of small misconfigurations. When you design on AWS, assume any one instance can be popped and ask "what can it reach, and would I see it?"

## Self-Test (answers at the bottom)
1. Decode the instance type `c5.2xlarge`: what family, and what workload is it for?
2. Why don't you need to write an outbound rule for the response to an allowed inbound request in a security group?
3. Your app server sits in a private subnet and must download OS patches. Which gateway lets it, and why doesn't that expose it to inbound attacks?
4. You want to add 2 instances automatically when CPU exceeds 70% for 3 minutes. Sketch the CloudWatch alarm and its action.
5. Design sketch: Draw a highly-available web app on AWS — ALB, 4 app servers, 1 primary + 1 replica database — placing each in the right subnet type across 2 AZs, and name every gateway and security-group rule you'd configure.

## Interview Soundbites
- "Public vs private subnet is entirely about the route table: a `0.0.0.0/0` route to the internet gateway makes it public; a route to a NAT gateway gives private instances outbound-only egress."
- "Security groups are stateful allow-lists — I chain them so the DB only accepts traffic from the app SG, which only accepts from the ALB SG; no raw IPs, no open ports."
- "I always span at least two Availability Zones — an AZ is a data center and data centers fail; single-AZ is a single point of failure dressed up as the cloud."

## Mini-Assignment
On paper (~30 min): Design the network for an e-commerce backend. (1) Choose a VPC CIDR and carve public + private subnets across two AZs — write the CIDR blocks. (2) Place an ALB, three app servers, and an RDS primary+replica in the correct subnets. (3) Write the three chained security-group rule sets (ALB, app, DB) with exact ports and sources. (4) Add a NAT gateway and state which subnet it lives in and what routes point to it. (5) Define one CloudWatch alarm (metric, threshold, action) that scales the app tier. Estimate the monthly NAT cost if the app egresses ~500 GB/month.

## Recap & Tomorrow
- **EC2:** on-demand virtual servers; `family.size` naming (m5.xlarge); On-Demand vs Reserved vs Spot pricing.
- **Security groups:** stateful, allow-only per-instance firewalls; chain them by referencing other SGs; NACLs for deny rules.
- **CloudWatch:** metrics + logs + alarms; drives paging and Auto Scaling; watch cost on custom metrics/logs.
- **VPC & subnets:** your private network; public subnet (route to IGW) vs private (route to NAT); span ≥2 AZs.
- **Gateways:** IGW = two-way front door for public subnets; NAT = outbound-only exit for private subnets.

Tomorrow we close Phase 1's cloud groundwork and move toward the core scaling building blocks — load balancing, caching, and database internals — where these AWS primitives become the canvas for real distributed-system design.

## Self-Test Answers
1. `c5.2xlarge`: family **c** = compute-optimized, generation 5, size 2xlarge (8 vCPU / 16 GB). It's for CPU-heavy work like video encoding, batch processing, or high-throughput application servers.
2. Security groups are stateful — when you allow an inbound connection, the SG automatically tracks it and permits the return traffic, so no matching outbound rule is required.
3. A NAT gateway (in a public subnet) lets the private instance make outbound connections and receive their responses, but it drops unsolicited inbound connections — so the server can pull patches while remaining unreachable from the internet.
4. Alarm on metric `CPUUtilization > 70%` for 3 consecutive 1-minute periods → state goes to ALARM → action triggers an Auto Scaling policy that adds 2 instances (and optionally an SNS notification to on-call).
5. VPC (e.g., 10.0.0.0/16) with public subnets in AZ-a and AZ-b holding the ALB (and NAT GW), and private subnets in both AZs holding the app servers and the RDS primary (AZ-a) + replica (AZ-b). SG rules: ALB allows 443 from 0.0.0.0/0; app SG allows 8080 from the ALB SG; DB SG allows 5432 from the app SG. Route tables: public subnets → IGW; private subnets → NAT GW for egress. Spanning two AZs keeps the app alive if one data center fails.
