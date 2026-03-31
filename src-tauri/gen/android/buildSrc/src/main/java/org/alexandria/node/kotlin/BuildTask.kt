import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction

open class BuildTask : DefaultTask() {
    private data class TauriCliCommand(
        val executable: String,
        val args: List<String>,
    )

    @Input
    var rootDirRel: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val rootDir = File(project.projectDir, rootDirRel)
        var lastException: Exception? = null

        for (command in tauriCliCommands(rootDir)) {
            try {
                runTauriCli(command, rootDir)
                return
            } catch (e: Exception) {
                lastException = e
            }
        }

        throw lastException ?: GradleException("Unable to find a usable Tauri CLI command")
    }

    private fun tauriCliCommands(rootDir: File): List<TauriCliCommand> {
        val commands = mutableListOf<TauriCliCommand>()
        val nodeModulesBin = File(rootDir, "node_modules/.bin")
        val tauriScriptNames = if (Os.isFamily(Os.FAMILY_WINDOWS)) {
            listOf("tauri.cmd", "tauri.exe", "tauri.bat")
        } else {
            listOf("tauri")
        }

        for (scriptName in tauriScriptNames) {
            val script = File(nodeModulesBin, scriptName)
            if (script.isFile) {
                commands += TauriCliCommand(
                    executable = script.absolutePath,
                    args = listOf("android", "android-studio-script"),
                )
            }
        }

        val cargoTauriNames = if (Os.isFamily(Os.FAMILY_WINDOWS)) {
            listOf("cargo-tauri.exe", "cargo-tauri.cmd", "cargo-tauri.bat")
        } else {
            listOf("cargo-tauri")
        }
        for (executable in cargoTauriNames) {
            commands += TauriCliCommand(
                executable = executable,
                args = listOf("android", "android-studio-script"),
            )
        }

        val cargoNames = if (Os.isFamily(Os.FAMILY_WINDOWS)) {
            listOf("cargo", "cargo.exe", "cargo.cmd", "cargo.bat")
        } else {
            listOf("cargo")
        }
        for (executable in cargoNames) {
            commands += TauriCliCommand(
                executable = executable,
                args = listOf("tauri", "android", "android-studio-script"),
            )
        }

        return commands
    }

    private fun runTauriCli(command: TauriCliCommand, rootDir: File) {
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        val args = command.args.toMutableList()

        project.exec {
            workingDir(rootDir)
            executable(command.executable)
            args(args)
            if (project.logger.isEnabled(LogLevel.DEBUG)) {
                args("-vv")
            } else if (project.logger.isEnabled(LogLevel.INFO)) {
                args("-v")
            }
            if (release) {
                args("--release")
            }
            args(listOf("--target", target))
        }.assertNormalExitValue()
    }
}
