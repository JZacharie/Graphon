```python
import os

readme_content = """# Graphon 🏛️

**Graphon** (du grec ancien *Graphéion*, le bâtiment officiel des archives publiques) est un outil d'indexation, de tri automatisé et de nettoyage pour Gmail. Conçu pour transformer une boîte de réception chaotique en une base de connaissances structurée, il prépare et organise vos données d'emails pour alimenter efficacement un système de **RAG (Retrieval-Augmented Generation)**.

L'objectif principal est de localiser instantanément vos documents et pièces jointes essentiels tout en maintenant votre boîte aux lettres saine et optimisée.

## 🚀 Objectifs du Projet

* **Indexation pour le RAG :** Extraction et structuration du texte des emails et des pièces jointes (PDF, DOCX, XLSX, etc.) pour permettre une recherche sémantique ultra-rapide.
* **Gestion Intelligente des Tags & Importance :** Classification automatique des messages entrants, application de labels contextuels et priorisation selon le niveau d'importance.
* **Nettoyage des Indésirables :** Identification et déplacement automatique des publicités, newsletters et spams en dehors de la boîte de réception principale.
* **Purge des Mails Périmés :** Règle de rétention stricte pour supprimer automatiquement les notifications éphémères, les codes de validation obsolètes ou les messages ayant dépassé leur date de validité.

## 🛠️ Architecture & Fonctionnalités


```

```text
README.md créé avec succès.


```

[ Gmail API ] ──> [ Graphon Engine ] ──> [ Base Documentaire / RAG ]
│
┌────────────────┴────────────────┐
▼                                 ▼
[ Tri & Nettoyage ]              [ Indexation & Tags ]

* Suppression des périmés        - Extraction des métadonnées
* Isolation des publicités        - Classification d'importance

```

### 1. Extraction & Parsing (Pipeline RAG)
* Connexion sécurisée via l'API Gmail (OAuth2).
* Extraction du contenu brut, nettoyage du HTML encombrant.
* Parsing avancé des pièces jointes pour les convertir en chunks de texte prêts à être vectorisés.

### 2. Moteur de Règles & Classification
* **Labels Dynamiques :** Application de tags basés sur le contexte, l'expéditeur ou le projet associé.
* **Détection Publicitaire :** Filtrage algorithmique des newsletters et des relances commerciales.

### 3. Cycle de Vie des Emails (Garbage Collector)
* Analyse des dates et des types de contenu.
* Règle de suppression automatique pour les emails considérés comme "périmés" (ex: confirmations de commande de plus d'un an, codes OTP de plus de 24h, alertes de supervision obsolètes).

## 📋 Prérequis

* Python 3.10+
* Un compte Google Cloud Platform (GCP) avec l'API Gmail activée.
* Un fichier de configurations d'identifiants `credentials.json` (OAuth2).

## 📦 Installation

1. Cloner le dépôt :
   ```bash
   git clone [https://github.com/votre-username/graphon.git](https://github.com/votre-username/graphon.git)
   cd graphon

```

2. Installer les dépendances :
```bash
pip install -r requirements.txt

```


3. Placer vos identifiants Google à la racine du projet sous le nom `credentials.json`.

## 🚦 Utilisation

Pour lancer l'indexation et le tri initial :

```bash
python main.py --sync --clean

```

### Options disponibles :

* `--sync` : Lance la synchronisation et l'indexation des nouveaux messages.
* `--clean` : Applique les règles de suppression des messages périmés et déplace les publicités.
* `--export-rag` : Génère les payloads JSON prêts à être ingérés par votre base vectorielle.

## 📄 Licence

Ce projet est sous licence MIT. Voir le fichier [LICENSE](https://www.google.com/search?q=LICENSE) pour plus de détails.
"""

with open("README.md", "w", encoding="utf-8") as f:
f.write(readme_content)

print("README.md créé avec succès.")

```
Votre fichier `README.md` pour le projet **Graphon** est prêt. Vous pouvez le télécharger ci-dessous :

[file-tag: code-generated-file-0-1779894467113612295]

Voici un aperçu du contenu généré pour documenter votre projet d'indexation Gmail et de préparation au RAG :

***

# Graphon 🏛️

**Graphon** (du grec ancien *Graphéion*, le bâtiment officiel des archives publiques) est un outil d'indexation, de tri automatisé et de nettoyage pour Gmail. Conçu pour transformer une boîte de réception chaotique en une base de connaissances structurée, il prépare et organise vos données d'emails pour alimenter efficacement un système de **RAG (Retrieval-Augmented Generation)**.

L'objectif principal est de localiser instantanément vos documents et pièces jointes essentiels tout en maintenant votre boîte aux lettres saine et optimisée.

## 🚀 Objectifs du Projet

* **Indexation pour le RAG :** Extraction et structuration du texte des emails et des pièces jointes (PDF, DOCX, XLSX, etc.) pour permettre une recherche sémantique ultra-rapide.
* **Gestion Intelligente des Tags & Importance :** Classification automatique des messages entrants, application de labels contextuels et priorisation selon le niveau d'importance.
* **Nettoyage des Indésirables :** Identification et déplacement automatique des publicités, newsletters et spams en dehors de la boîte de réception principale.
* **Purge des Mails Périmés :** Règle de rétention stricte pour supprimer automatiquement les notifications éphémères, les codes de validation obsolètes ou les messages ayant dépassé leur date de validité.

## 🛠️ Architecture & Fonctionnalités


```

[ Gmail API ] ──> [ Graphon Engine ] ──> [ Base Documentaire / RAG ]
│
┌────────────────┴────────────────┐
▼                                 ▼
[ Tri & Nettoyage ]              [ Indexation & Tags ]

* Suppression des périmés        - Extraction des métadonnées
* Isolation des publicités        - Classification d'importance

```

### 1. Extraction & Parsing (Pipeline RAG)
* Connexion sécurisée via l'API Gmail (OAuth2).
* Extraction du contenu brut, nettoyage du HTML encombrant.
* Parsing avancé des pièces jointes pour les convertir en chunks de texte prêts à être vectorisés.

### 2. Moteur de Règles & Classification
* **Labels Dynamiques :** Application de tags basés sur le contexte, l'expéditeur ou le projet associé.
* **Détection Publicitaire :** Filtrage algorithmique des newsletters et des relances commerciales.

### 3. Cycle de Vie des Emails (Garbage Collector)
* Analyse des dates et des types de contenu.
* Règle de suppression automatique pour les emails considérés comme "périmés" (ex: confirmations de commande de plus d'un an, codes OTP de plus de 24h, alertes de supervision obsolètes).

## 📋 Prérequis

* Python 3.10+
* Un compte Google Cloud Platform (GCP) avec l'API Gmail activée.
* Un fichier de configurations d'identifiants `credentials.json` (OAuth2).

## 📦 Installation

1. Cloner le dépôt :
   ```bash
   git clone https://github.com/votre-username/graphon.git
   cd graphon

```

2. Installer les dépendances :
```bash
pip install -r requirements.txt

```


3. Placer vos identifiants Google à la racine du projet sous le nom `credentials.json`.

## 🚦 Utilisation

Pour lancer l'indexation et le tri initial :

```bash
python main.py --sync --clean

```

### Options disponibles :

* `--sync` : Lance la synchronisation et l'indexation des nouveaux messages.
* `--clean` : Applique les règles de suppression des messages périmés et déplace les publicités.
* `--export-rag` : Génère les payloads JSON prêts à être ingérés par votre base vectorielle.
