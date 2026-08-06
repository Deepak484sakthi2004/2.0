# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 16 of 63 — Phase 1: Foundations (Module 16 of 28)
**Module:** Kubernetes Deep Dive
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
In Lesson 13 you learned a container starts in milliseconds and packs densely. But what runs *ten thousand* of them across 500 machines, restarts the ones that crash, scales them up on a traffic spike at midnight, and routes users to healthy ones? That's **Kubernetes** — the operating system of the cloud. It grew out of Google's internal Borg, which schedules billions of containers a week. Almost every company hiring at SDE2/SDE3 runs on K8s or something like it, and "how does your deployment self-heal and autoscale?" is a standard interview probe. Master the control plane, pods, services, and autoscaling and you can reason about how any modern cloud system actually stays up.

## Learning Objectives
By the end of this lesson you can:
- Name every control-plane and node component and say what each one does.
- Explain the reconciliation loop — Kubernetes' core "desired vs actual state" mechanism.
- Distinguish Pods, Deployments, and the three Service types with when to use each.
- Trace how HPA autoscaling decides to add replicas using a real CPU-based formula.
- Describe a rolling update and how K8s achieves zero-downtime deploys.

## The Lesson

### Control Plane & Nodes
**What it is (plain English):** A Kubernetes cluster has two kinds of machines: the **control plane** (the brain that decides what should run where) and **worker nodes** (the muscle that actually runs your containers). You tell the brain *what you want*; it makes the nodes match.

**The problem it solves:** Managing thousands of containers by hand is impossible. The control plane centralizes all scheduling, health-tracking, and recovery decisions so humans declare intent instead of issuing commands.

**How it works (mechanics):** The control plane has four core components; each node has three:
```
CONTROL PLANE (brain)
  [ API Server ]  ← the front door; everything talks through it
  [ etcd ]        ← the database; stores the whole cluster state (key-value)
  [ Scheduler ]   ← picks which node a new Pod runs on
  [ Controller Mgr]← runs reconciliation loops (desired vs actual)

WORKER NODE (muscle)  ×N
  [ kubelet ]     ← agent that starts/stops containers, reports health
  [ kube-proxy ]  ← programs networking/load-balancing rules
  [ container runtime ] ← (containerd) actually runs containers
```
The heart is the **reconciliation loop**: you declare "I want 5 replicas" → stored in **etcd** → a controller continuously compares desired (5) vs actual (say 4 after a crash) → tells the scheduler to place one more → kubelet starts it. This runs forever, every few seconds. Worked number: kubelet posts heartbeats; miss them for ~40 s and the node is marked NotReady, its Pods rescheduled elsewhere.

**Trade-offs / when NOT to use it:** etcd is the SPOF of the cluster — lose it, lose the cluster's memory (run 3–5 nodes for quorum). The control plane is operationally heavy; for a handful of services it's overkill.

**Where you'll see it:** Every managed K8s — GKE, EKS, AKS — hides the control plane and charges you for nodes.

### Pods, Services, Deployments
**What it is (plain English):** The three objects you'll use daily. A **Pod** is the smallest unit — one or more tightly-coupled containers sharing a network address. A **Deployment** manages many identical Pods (scaling, updates, self-healing). A **Service** gives those ever-changing Pods one stable address to reach them by.

**The problem it solves:** Pods are mortal and get new IPs when they restart, so you can't hardwire to a Pod. Deployments keep the right number alive; Services provide a stable front so callers don't chase moving IPs.

**How it works (mechanics):**
```
[ Deployment: replicas=3, image=api:v2 ]
        │ creates & maintains
   ┌────┼────┐
 [Pod] [Pod] [Pod]   ← each an app instance, own IP (10.1.x.x)
   └────┼────┘
[ Service: api ]  stable ClusterIP 10.96.0.5 → load-balances to the 3 Pods
```
A **Deployment** declares "3 Pods of `api:v2`"; if a Pod dies, the controller spawns a replacement (self-healing). A **Service** watches which Pods are healthy (via label selectors + readiness probes) and load-balances across them. Three Service types: **ClusterIP** (internal-only, default), **NodePort** (opens a port on every node), **LoadBalancer** (provisions a cloud LB with a public IP). Worked example: Service `api` at `10.96.0.5:80` fans traffic to Pods `10.1.1.4`, `10.1.2.7`, `10.1.3.9`; kill one and the Service drops it from rotation within a readiness-probe cycle (~seconds).

**Trade-offs / when NOT to use it:** Cramming unrelated containers into one Pod couples their lifecycle (they scale and die together) — usually wrong; one main container per Pod is the norm. LoadBalancer per service gets expensive (one cloud LB each) — use an Ingress to share one.

**Where you'll see it:** Literally every K8s app manifest; this is the day-to-day vocabulary of cloud deployment.

### Autoscaling
**What it is (plain English):** Kubernetes automatically changing capacity to match load — more Pods when busy, fewer when idle, and even more *nodes* when the Pods won't fit. Three flavors: **HPA** (horizontal — more Pods), **VPA** (vertical — bigger Pods), and **Cluster Autoscaler** (more nodes).

**The problem it solves:** Traffic is spiky. Provisioning for peak wastes money 90% of the day; provisioning for average falls over at peak. Autoscaling tracks demand so you pay for what you use and survive spikes.

**How it works (mechanics):** The **Horizontal Pod Autoscaler** runs a control loop (default every 15 s) using a simple formula:
```
desiredReplicas = ceil( currentReplicas × (currentMetric / targetMetric) )
```
Worked example: target CPU = 50%, you run 4 Pods currently averaging 90% CPU:
```
desired = ceil( 4 × (90 / 50) ) = ceil(7.2) = 8 Pods
```
HPA scales 4 → 8. Traffic later drops to 20% CPU: `ceil(8 × 20/50) = ceil(3.2) = 4` → scales back to 4 (with a cooldown to avoid flapping). If those new Pods can't fit on existing nodes, the **Cluster Autoscaler** adds a node (takes minutes — cloud VM provisioning), then the scheduler places the pending Pods.

**Trade-offs / when NOT to use it:** Autoscaling reacts, so a sudden 10x spike still hits before new Pods are ready (cold start + node provisioning lag) — pre-warm or over-provision headroom for known spikes. Scaling on CPU can mislead for I/O-bound apps; scale on the right metric (QPS, queue depth). Flapping wastes churn if thresholds are too tight.

**Where you'll see it:** Every elastic cloud service — Black Friday retail, streaming launch nights; combined with Cluster Autoscaler on GKE/EKS.

### Container Orchestration for Cloud Deployment
**What it is (plain English):** The whole job Kubernetes does — scheduling, healing, scaling, networking, and rolling out new versions of containers across a fleet — so you deploy *declaratively* (describe the end state) instead of *imperatively* (script every step). The marquee feature for teams is the **zero-downtime rolling update**.

**The problem it solves:** Deploying a new version across 50 machines by hand risks downtime and inconsistency. Orchestration makes deploys safe, repeatable, and reversible, and keeps the system in its desired state without babysitting.

**How it works (mechanics):** A **rolling update** replaces old Pods with new ones gradually, keeping the service up throughout:
```
Start: [v1][v1][v1][v1]   (4 replicas)
Step1: [v2][v1][v1][v1]   add one v2, wait for readiness probe OK
Step2: [v2][v2][v1][v1]   then kill one v1
...    [v2][v2][v2][v2]   done — never below healthy capacity
```
Governed by `maxUnavailable` (how many can be down) and `maxSurge` (how many extra during rollout). A **readiness probe** gates traffic — a new Pod gets zero requests until it passes. If v2 crashes or fails probes, `kubectl rollout undo` **rolls back** to v1 in seconds because the old ReplicaSet is retained. Worked number: maxSurge=1, maxUnavailable=0 on 4 replicas means capacity never dips below 4 during the deploy.

**Trade-offs / when NOT to use it:** Rolling updates briefly run *both* versions simultaneously — your API and DB schema must be backward-compatible for that window, or you break in-flight requests. For risky changes prefer blue-green or canary. Stateful apps need StatefulSets and care (stable identity, ordered rollout).

**Where you'll see it:** Every CI/CD pipeline deploying to K8s — Spotify, Airbnb, Shopify — plus GitOps tools (Argo CD, Flux) that drive it declaratively.

## Comparison Table

| Dimension | Pod | Deployment | Service |
|---|---|---|---|
| What it is | Smallest runnable unit (containers) | Manages N identical Pods | Stable network endpoint |
| Lifespan | Ephemeral (gets new IP on restart) | Long-lived controller | Long-lived, stable IP |
| Self-heals? | No (dies and is gone) | Yes (recreates Pods) | N/A (routes to healthy Pods) |
| Scales? | No (one instance) | Yes (replicas / HPA) | Load-balances across replicas |
| You deploy | Rarely directly | Yes (the usual object) | Yes (to expose Pods) |

**Verdict:** You almost never create bare Pods — declare a Deployment for the workload and a Service to reach it; let controllers do the healing.

## Common Misconceptions
- **Myth:** A Pod is the same as a container. → **Reality:** A Pod wraps one or more containers sharing a network namespace/IP; usually one main container per Pod.
- **Myth:** You should create Pods directly. → **Reality:** You create Deployments; they manage Pods and give you healing, scaling, and rollbacks.
- **Myth:** Services store or run your app. → **Reality:** A Service is just a stable virtual IP + load-balancing rule pointing at Pods; it runs nothing.
- **Myth:** Autoscaling handles any spike instantly. → **Reality:** New Pods and especially new nodes take seconds-to-minutes; sudden spikes need headroom or pre-warming.
- **Myth:** Kubernetes guarantees zero downtime automatically. → **Reality:** Only if you set readiness probes and keep schema/API changes backward-compatible during rollout.

## Real-World Case
Kubernetes descends directly from Google's internal **Borg**, which has scheduled Google's workloads for over 15 years — running billions of containers per week across clusters of tens of thousands of machines. Borg's key lesson, carried into K8s, was *declarative reconciliation*: engineers describe the desired state and a control loop relentlessly drives reality toward it, so a machine dying at 3 AM triggers no page — the controller just reschedules the Pods. When Google open-sourced these ideas as Kubernetes in 2014, it handed the industry Google-grade orchestration for free. The payoff shows up in incidents like large node failures where, instead of an outage, the cluster silently re-places hundreds of Pods within seconds. The design principle to internalize: *converge toward desired state continuously*, rather than react to each failure by hand.

## Self-Test (answers at the bottom)
1. Name the four control-plane components and, in a few words each, what they do.
2. Why can't you point a client directly at a Pod's IP, and what object solves this?
3. HPA targets 60% CPU. You run 6 Pods averaging 85% CPU. How many replicas does HPA compute? Show the formula.
4. During a rolling update with maxUnavailable=0 and maxSurge=1 on 4 replicas, what is the minimum number of healthy Pods at any moment, and how does a new Pod avoid getting traffic before it's ready?
5. Design sketch: You deploy a stateless REST API expecting 2,000 QPS normally and 20,000 QPS during a daily flash sale. Describe the K8s objects and autoscaling config you'd use, one risk of relying purely on autoscaling for the spike, and how you'd mitigate it.

## Interview Soundbites
- "Kubernetes is a reconciliation engine: you declare desired state in etcd, and control loops relentlessly drive actual state to match — that's why it self-heals without a human."
- "Pods are cattle, not pets — they're ephemeral with changing IPs, so I front them with a Service for a stable endpoint and let a Deployment keep the replica count right."
- "Rolling updates give zero downtime only when readiness probes gate traffic and my schema stays backward-compatible for the window where v1 and v2 run side by side."

## Mini-Assignment
On paper (~30 min): (1) Sketch the full request path for `curl http://api-service` inside a cluster: which components and objects does it traverse to reach a healthy Pod? (2) Write an HPA spec in plain English: target metric, min/max replicas, and trace what happens as CPU goes 30% → 80% → 30% over an hour using the scaling formula. (3) Describe how you'd deploy `v2` of the API with zero downtime and what one backward-compatibility issue could still break in-flight requests during the rollout.

## Recap & Tomorrow
- **Control plane & nodes:** API server, etcd, scheduler, controller manager (brain) + kubelet, kube-proxy, runtime (muscle); driven by the reconciliation loop.
- **Pods, Services, Deployments:** ephemeral Pods, Deployments that heal/scale them, Services (ClusterIP/NodePort/LoadBalancer) for stable addressing.
- **Autoscaling:** HPA (more Pods, via the CPU/metric formula), VPA (bigger Pods), Cluster Autoscaler (more nodes).
- **Orchestration:** declarative deploys with zero-downtime rolling updates, probes, and instant rollback.

Tomorrow we close out the deployment arc and move into **Module 17**, shifting from *where code runs* to the networking and communication layer that ties these services together.

## Self-Test Answers
1. **API Server** — the front door all components and users talk to. **etcd** — the key-value store holding the entire cluster state. **Scheduler** — decides which node each new Pod runs on. **Controller Manager** — runs reconciliation loops comparing desired vs actual state and driving them together.
2. A Pod is ephemeral and gets a new IP whenever it restarts or reschedules, so a hardcoded Pod IP goes stale. A **Service** provides a stable virtual IP (ClusterIP) that load-balances to the current healthy Pods.
3. `desired = ceil(6 × (85 / 60)) = ceil(8.5) = 9` replicas.
4. Minimum healthy Pods = 4 (maxUnavailable=0 means it never drops below the desired count; maxSurge=1 adds a temporary extra during the swap). A new Pod avoids premature traffic because its **readiness probe** must pass before the Service adds it to the load-balancing rotation.
5. A **Deployment** for the stateless API (min replicas sized for 2,000 QPS) fronted by a **Service** (LoadBalancer or via Ingress). An **HPA** scaling on CPU or QPS with a high max (enough for 20,000 QPS), plus **Cluster Autoscaler** for extra nodes. Risk: a sudden 10x spike arrives before new Pods/nodes are ready (provisioning lag), causing errors. Mitigation: pre-scale before the known sale window (scheduled scaling), keep warm headroom, and/or use a queue/rate-limit to absorb the burst.
