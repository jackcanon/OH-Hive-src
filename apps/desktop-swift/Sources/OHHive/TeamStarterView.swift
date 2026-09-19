import SwiftUI

struct TeamStarterView: View {
    let model: BotsModel
    @Environment(\.dismiss) private var dismiss
    @State private var setup: TeamStarter
    @State private var recipeName = ""
    private let context: String

    init(model: BotsModel) {
        self.model = model
        let context = model.teamContext
        self.context = context
        // Intentional one-time seed: a sheet owns one setup for one fleet/host identity.
        _setup = State(initialValue: TeamStarter(context: context))
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Build your team").font(.title2)
                Spacer()
                Button("Done") { dismiss() }.disabled(setup.busy)
            }
            Text("Start with suggestions or make them your own. Every name, responsibility and avatar is editable.")
            Text("These agents use this Mac’s configured local model, even when your fleet is stored on another computer. Creating profiles does not download or run a model. Tools are connected separately.")
                .font(.callout).foregroundStyle(.secondary)
            if !setup.started {
                HStack {
                    Button("Suggested team") { setup.choose(AgentRoleTemplate.all.filter { $0.id != "assistant-v1" }) }
                    Button("Choose agents") { setup.choose(AgentRoleTemplate.all, included: false) }
                    Button("One assistant") { setup.choose(Array(AgentRoleTemplate.all.prefix(1))) }
                }
                if !setup.recipes.isEmpty {
                    Menu("Use a saved team") {
                        ForEach(setup.recipes) { recipe in Button(recipe.name) { setup.use(recipe) } }
                    }
                }
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach($setup.members) { $member in
                        VStack(alignment: .leading, spacing: 8) {
                            HStack {
                                Toggle("Include", isOn: $member.included).labelsHidden().accessibilityLabel("Include \(member.name)")
                                TextField("Agent name", text: $member.name)
                                if member.complete { Label("Created", systemImage: "checkmark.circle") }
                            }
                            if member.included {
                                TextField("What does this agent help with?", text: $member.bio, axis: .vertical)
                                DisclosureGroup("Customize instructions and avatar") {
                                    TextEditor(text: $member.instructions).frame(height: 110).accessibilityLabel("Instructions for \(member.name)")
                                    Picker("Avatar", selection: $member.avatar) {
                                        ForEach(AgentAvatar.choices, id: \.self) { Text($0.capitalized).tag($0) }
                                    }
                                    Text("You can also upload your own image in the agent’s profile.").font(.caption)
                                }
                            }
                        }.padding(12).background(.quaternary, in: RoundedRectangle(cornerRadius: 10))
                            .disabled(setup.started || setup.busy)
                    }
                    if !setup.started {
                        Button("Add a custom agent", systemImage: "plus") {
                            var draft = TeamMemberDraft(template: AgentRoleTemplate.all[0])
                            draft.name = "My agent"; draft.bio = ""; draft.instructions = ""
                            setup.members.append(draft)
                        }
                    }
                }
            }
            if let error = setup.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
            if !setup.progress.isEmpty { Text(setup.progress).font(.callout) }
            HStack {
                TextField("Save this team as…", text: $recipeName)
                Button("Save preset") {
                    do { try setup.saveRecipe(name: recipeName); recipeName = "" }
                    catch { setup.error = error.localizedDescription }
                }.disabled(setup.busy || recipeName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            HStack {
                Text("\(setup.members.filter(\.included).count) agents selected").foregroundStyle(.secondary)
                Spacer()
                if setup.started && setup.members.filter(\.included).allSatisfy(\.complete) {
                    Button("Start another team") { setup.newTeam() }
                } else {
                    Button(setup.busy ? "Creating…" : setup.started ? "Continue setup" : "Create agents") {
                        Task {
                            await setup.create(createAgent: { name in
                                try await model.createStarterAgent(name: name, context: context)
                            }, saveProfile: { id, draft in
                                try await model.saveStarterProfile(id: id, draft: draft, context: context)
                            })
                            await model.refreshAgents()
                        }
                    }.buttonStyle(.borderedProminent)
                        .disabled(!setup.canCreate || !model.paired || model.teamContext != context)
                }
            }
        }.padding(24).frame(minWidth: 620, idealWidth: 720, minHeight: 560, idealHeight: 720)
            .interactiveDismissDisabled(setup.busy)
    }
}
